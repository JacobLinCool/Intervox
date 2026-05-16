// Intervox — Core Audio HAL AudioServerPlugIn (spec §9).
//
// Exposes one virtual INPUT device "Intervox" (48 kHz, mono, Float32). The
// realtime read path pulls 48k mono Float32 frames from the shared-memory
// ring buffer produced by the app and copies them into the host's input
// buffer; it outputs silence when the producer is absent or the buffer is
// empty (non-negotiable rules §19.4 never block, §19.5 silence when app gone).
//
// Structure follows Apple's "NullAudio" AudioServerPlugIn sample, trimmed to a
// single input device with no controls. The driver NEVER talks to OpenAI or
// the network (§19.3) and NEVER allocates on the IO path (§19.4).

#include <CoreAudio/AudioHardware.h>
#include <CoreAudio/AudioServerPlugIn.h>
#include <dispatch/dispatch.h>
#include <errno.h>
#include <mach/mach_time.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <sys/stat.h>
#include <syslog.h>

#include "intervox_ring.h"

#define LOG(fmt, ...) \
    do { syslog(LOG_NOTICE, "[Intervox] " fmt, ##__VA_ARGS__); } while (0)

// syslog from the coreaudiod-sandboxed plugin is not visible via `log show`,
// so the non-realtime poller thread mirrors driver state to a file. The RT
// path only touches atomics (no file IO).
#define IVX_DIAG_PATH "/tmp/intervox_driver.log"
static _Atomic unsigned long gDiag_DoIO = 0;       // RT increments
static _Atomic unsigned long gDiag_DoIOWithRing = 0;// RT increments
static _Atomic unsigned long gDiag_StartIO = 0;
static _Atomic unsigned long gDiag_StopIO = 0;
static _Atomic unsigned long gDiag_InitCalls = 0;
static _Atomic int gDiag_LastFrames = 0;
static _Atomic int gDiag_LastShmErrno = 0;   // errno of last failed shm_open
static _Atomic int gDiag_LastMapErrno = 0;   // errno of last failed mmap
static _Atomic unsigned long gDiag_ShmOpenOK = 0;

#pragma mark - Object IDs / constants

enum {
    kObjectID_PlugIn = kAudioObjectPlugInObject, // 1
    kObjectID_Device = 2,
    kObjectID_Stream_Input = 3,
};

#define kDevice_UID          "IntervoxDevice:1"
#define kDevice_ModelUID     "IntervoxModel:1"
#define kDevice_Name         "Intervox"
#define kManufacturer_Name   "Intervox"
#define kPlugIn_BundleID     "app.intervox.driver"

#define kSampleRate          48000.0
#define kChannelsPerFrame    1u
#define kBitsPerChannel      32u
#define kBytesPerFrame       4u
// Host requests this many frames as the zero-timestamp period.
#define kRingPeriodFrames    2048u

#pragma mark - Driver state

static pthread_mutex_t gStateMutex = PTHREAD_MUTEX_INITIALIZER;
static UInt32 gPlugInRefCount = 1;
static AudioServerPlugInHostRef gHost = NULL;

static UInt32 gDevice_IORunningClients = 0;
static Float64 gDevice_SampleRate = kSampleRate;
static bool gStream_Active = true;

// Zero-timestamp anchor (NullAudio model).
static Float64 gHostTicksPerFrame = 0.0;
static UInt64 gAnchorHostTime = 0;
static UInt64 gNumberTimeStamps = 0;

// Shared-memory ring buffer. A background poller (NOT the realtime IO thread)
// owns mapping/unmapping and publishes the pointer atomically; the IO path
// only does an atomic load (no syscalls, no allocation — rule §19.4). This
// makes the device robust to the app starting AFTER the meeting app already
// opened the device, and to the producer restarting (new shm inode).
static _Atomic(intervox_ring_t*) gRing = NULL; // RT reads this (atomic load)
static int gRingFD = -1;                       // poller-thread only
static ino_t gRingIno = 0;                     // poller-thread only

// One-slot deferred reclamation. ALL munmap/close happen on the poller
// thread, and only one full poll tick (≥250 ms) AFTER the pointer was
// unpublished from gRing. A DoIOOperation read lasts microseconds and only
// occurs between Begin/EndIOOperation, so by free time no RT thread can still
// hold the retired pointer — no lock needed on the realtime path.
static intervox_ring_t* gPendingFree = NULL;   // poller-thread only
static int gPendingFreeFD = -1;                // poller-thread only

static _Atomic bool gIOActive = false;
static _Atomic bool gPollerRun = false;
static pthread_t gPollerThread;

// poller-thread only. Retire the currently published mapping (already assumes
// gPendingFree was flushed at the top of this tick).
static void Ring_Retire(void) {
    intervox_ring_t* old = atomic_exchange_explicit(&gRing, NULL,
                                                    memory_order_acq_rel);
    if (old != NULL) {
        gPendingFree = old;
        gPendingFreeFD = gRingFD;
    }
    gRingFD = -1;
    gRingIno = 0;
}

// poller-thread only: snapshot driver state to a file for debugging. Disabled
// unless the sentinel /tmp/intervox.diag exists, so production never writes to
// /tmp. Enable with: touch /tmp/intervox.diag
#define IVX_DIAG_SENTINEL "/tmp/intervox.diag"
static void Diag_Write(const char* phase) {
    if (access(IVX_DIAG_SENTINEL, F_OK) != 0) {
        return;
    }
    FILE* f = fopen(IVX_DIAG_PATH, "w");
    if (f == NULL) {
        return;
    }
    intervox_ring_t* rb = atomic_load(&gRing);
    fprintf(f,
            "phase=%s pollerRun=%d ioActive=%d ring=%p ringIno=%llu "
            "init=%lu startIO=%lu stopIO=%lu doIO=%lu doIOWithRing=%lu "
            "lastFrames=%d clients=%u shmOpenOK=%lu shmErrno=%d mapErrno=%d\n",
            phase, atomic_load(&gPollerRun), atomic_load(&gIOActive),
            (void*)rb, (unsigned long long)gRingIno,
            atomic_load(&gDiag_InitCalls), atomic_load(&gDiag_StartIO),
            atomic_load(&gDiag_StopIO), atomic_load(&gDiag_DoIO),
            atomic_load(&gDiag_DoIOWithRing), atomic_load(&gDiag_LastFrames),
            gDevice_IORunningClients, atomic_load(&gDiag_ShmOpenOK),
            atomic_load(&gDiag_LastShmErrno), atomic_load(&gDiag_LastMapErrno));
    if (rb != NULL) {
        fprintf(f, "ring magic=0x%x ver=%u sr=%u ch=%u w=%llu r=%llu\n",
                rb->magic, rb->version, rb->sample_rate, rb->channels,
                (unsigned long long)atomic_load_explicit(
                    &rb->write_index, memory_order_relaxed),
                (unsigned long long)atomic_load_explicit(
                    &rb->read_index, memory_order_relaxed));
    }
    fclose(f);
}

// Background thread: owns all mapping. (Re)maps the shared object when it
// appears or its identity changes (producer restart -> new inode); drops the
// mapping when the producer is gone or the device went idle, so the realtime
// path cleanly outputs silence (rules §19.4 no syscalls on IO path, §19.5
// silence when app gone).
static void* Ring_PollerMain(void* arg) {
    (void)arg;
    while (atomic_load(&gPollerRun)) {
        // 1) Free anything retired on a previous tick (now safe).
        if (gPendingFree != NULL) {
            intervox_ring_close(gPendingFree, gPendingFreeFD);
            gPendingFree = NULL;
            gPendingFreeFD = -1;
        }

        if (atomic_load(&gIOActive)) {
            int fd = shm_open(INTERVOX_SHM_NAME, O_RDWR, 0666);
            if (fd < 0) {
                atomic_store(&gDiag_LastShmErrno, errno);
                if (atomic_load(&gRing) != NULL) {
                    Ring_Retire(); // producer gone -> silence
                    LOG("poller: producer gone, mapping retired");
                }
            } else {
                atomic_fetch_add(&gDiag_ShmOpenOK, 1);
                struct stat st;
                bool need =
                    (fstat(fd, &st) == 0) &&
                    (atomic_load(&gRing) == NULL || st.st_ino != gRingIno);
                if (need && gPendingFree == NULL) {
                    void* p = mmap(NULL, sizeof(intervox_ring_t),
                                   PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
                    if (p == MAP_FAILED) {
                        atomic_store(&gDiag_LastMapErrno, errno);
                    }
                    if (p != MAP_FAILED) {
                        intervox_ring_t* rb = (intervox_ring_t*)p;
                        if (rb->magic == INTERVOX_RING_MAGIC &&
                            rb->version == INTERVOX_RING_VERSION) {
                            Ring_Retire();
                            gRingFD = fd;
                            gRingIno = st.st_ino;
                            atomic_store_explicit(&gRing, rb,
                                                  memory_order_release);
                            fd = -1; // ownership moved to the live mapping
                            LOG("poller: mapped ring (ino=%llu)",
                                (unsigned long long)st.st_ino);
                        } else {
                            munmap(p, sizeof(intervox_ring_t));
                        }
                    }
                }
                if (fd >= 0) {
                    close(fd);
                }
            }
        } else if (atomic_load(&gRing) != NULL) {
            Ring_Retire(); // device idle -> drop mapping
        }
        Diag_Write("poll");
        usleep(250000); // 250 ms — well off the realtime path
    }
    return NULL;
}

#pragma mark - forward decls

static HRESULT Ivx_QueryInterface(void* inDriver, REFIID inUUID, LPVOID* outIface);
static ULONG Ivx_AddRef(void* inDriver);
static ULONG Ivx_Release(void* inDriver);
static OSStatus Ivx_Initialize(AudioServerPlugInDriverRef inDriver,
                               AudioServerPlugInHostRef inHost);
static OSStatus Ivx_CreateDevice(AudioServerPlugInDriverRef inDriver,
                                 CFDictionaryRef inDescription,
                                 const AudioServerPlugInClientInfo* inClientInfo,
                                 AudioObjectID* outDeviceObjectID);
static OSStatus Ivx_DestroyDevice(AudioServerPlugInDriverRef inDriver,
                                  AudioObjectID inDeviceObjectID);
static OSStatus Ivx_AddDeviceClient(AudioServerPlugInDriverRef inDriver,
                                    AudioObjectID inDeviceObjectID,
                                    const AudioServerPlugInClientInfo* inClientInfo);
static OSStatus Ivx_RemoveDeviceClient(AudioServerPlugInDriverRef inDriver,
                                       AudioObjectID inDeviceObjectID,
                                       const AudioServerPlugInClientInfo* inClientInfo);
static OSStatus Ivx_PerformDeviceConfigurationChange(AudioServerPlugInDriverRef inDriver,
                                                     AudioObjectID inDeviceObjectID,
                                                     UInt64 inChangeAction,
                                                     void* inChangeInfo);
static OSStatus Ivx_AbortDeviceConfigurationChange(AudioServerPlugInDriverRef inDriver,
                                                   AudioObjectID inDeviceObjectID,
                                                   UInt64 inChangeAction,
                                                   void* inChangeInfo);
static Boolean Ivx_HasProperty(AudioServerPlugInDriverRef inDriver,
                               AudioObjectID inObjectID, pid_t inClientPID,
                               const AudioObjectPropertyAddress* inAddress);
static OSStatus Ivx_IsPropertySettable(AudioServerPlugInDriverRef inDriver,
                                       AudioObjectID inObjectID, pid_t inClientPID,
                                       const AudioObjectPropertyAddress* inAddress,
                                       Boolean* outIsSettable);
static OSStatus Ivx_GetPropertyDataSize(AudioServerPlugInDriverRef inDriver,
                                        AudioObjectID inObjectID, pid_t inClientPID,
                                        const AudioObjectPropertyAddress* inAddress,
                                        UInt32 inQualifierDataSize,
                                        const void* inQualifierData,
                                        UInt32* outDataSize);
static OSStatus Ivx_GetPropertyData(AudioServerPlugInDriverRef inDriver,
                                    AudioObjectID inObjectID, pid_t inClientPID,
                                    const AudioObjectPropertyAddress* inAddress,
                                    UInt32 inQualifierDataSize,
                                    const void* inQualifierData,
                                    UInt32 inDataSize, UInt32* outDataSize,
                                    void* outData);
static OSStatus Ivx_SetPropertyData(AudioServerPlugInDriverRef inDriver,
                                    AudioObjectID inObjectID, pid_t inClientPID,
                                    const AudioObjectPropertyAddress* inAddress,
                                    UInt32 inQualifierDataSize,
                                    const void* inQualifierData,
                                    UInt32 inDataSize, const void* inData);
static OSStatus Ivx_StartIO(AudioServerPlugInDriverRef inDriver,
                            AudioObjectID inDeviceObjectID, UInt32 inClientID);
static OSStatus Ivx_StopIO(AudioServerPlugInDriverRef inDriver,
                           AudioObjectID inDeviceObjectID, UInt32 inClientID);
static OSStatus Ivx_GetZeroTimeStamp(AudioServerPlugInDriverRef inDriver,
                                     AudioObjectID inDeviceObjectID,
                                     UInt32 inClientID, Float64* outSampleTime,
                                     UInt64* outHostTime, UInt64* outSeed);
static OSStatus Ivx_WillDoIOOperation(AudioServerPlugInDriverRef inDriver,
                                      AudioObjectID inDeviceObjectID,
                                      UInt32 inClientID, UInt32 inOperationID,
                                      Boolean* outWillDo,
                                      Boolean* outWillDoInPlace);
static OSStatus Ivx_BeginIOOperation(AudioServerPlugInDriverRef inDriver,
                                     AudioObjectID inDeviceObjectID,
                                     UInt32 inClientID, UInt32 inOperationID,
                                     UInt32 inIOBufferFrameSize,
                                     const AudioServerPlugInIOCycleInfo* inIOCycleInfo);
static OSStatus Ivx_DoIOOperation(AudioServerPlugInDriverRef inDriver,
                                  AudioObjectID inDeviceObjectID,
                                  AudioObjectID inStreamObjectID,
                                  UInt32 inClientID, UInt32 inOperationID,
                                  UInt32 inIOBufferFrameSize,
                                  const AudioServerPlugInIOCycleInfo* inIOCycleInfo,
                                  void* ioMainBuffer, void* ioSecondaryBuffer);
static OSStatus Ivx_EndIOOperation(AudioServerPlugInDriverRef inDriver,
                                   AudioObjectID inDeviceObjectID,
                                   UInt32 inClientID, UInt32 inOperationID,
                                   UInt32 inIOBufferFrameSize,
                                   const AudioServerPlugInIOCycleInfo* inIOCycleInfo);

#pragma mark - Interface vtable + COM glue

static AudioServerPlugInDriverInterface gInterface = {
    NULL,
    Ivx_QueryInterface,
    Ivx_AddRef,
    Ivx_Release,
    Ivx_Initialize,
    Ivx_CreateDevice,
    Ivx_DestroyDevice,
    Ivx_AddDeviceClient,
    Ivx_RemoveDeviceClient,
    Ivx_PerformDeviceConfigurationChange,
    Ivx_AbortDeviceConfigurationChange,
    Ivx_HasProperty,
    Ivx_IsPropertySettable,
    Ivx_GetPropertyDataSize,
    Ivx_GetPropertyData,
    Ivx_SetPropertyData,
    Ivx_StartIO,
    Ivx_StopIO,
    Ivx_GetZeroTimeStamp,
    Ivx_WillDoIOOperation,
    Ivx_BeginIOOperation,
    Ivx_DoIOOperation,
    Ivx_EndIOOperation,
};
static AudioServerPlugInDriverInterface* gInterfacePtr = &gInterface;
static AudioServerPlugInDriverRef gDriverRef = &gInterfacePtr;

// CFPlugIn factory — referenced by Info.plist CFPlugInFactories.
void* IntervoxCreate(CFAllocatorRef inAllocator, CFUUIDRef inRequestedTypeUUID);
void* IntervoxCreate(CFAllocatorRef inAllocator, CFUUIDRef inRequestedTypeUUID) {
    (void)inAllocator;
    if (!CFEqual(inRequestedTypeUUID, kAudioServerPlugInTypeUUID)) {
        return NULL;
    }
    return gDriverRef;
}

static HRESULT Ivx_QueryInterface(void* inDriver, REFIID inUUID, LPVOID* outIface) {
    if (inDriver != gDriverRef || outIface == NULL) {
        return E_INVALIDARG;
    }
    CFUUIDRef req = CFUUIDCreateFromUUIDBytes(NULL, inUUID);
    Boolean ok = CFEqual(req, IUnknownUUID) ||
                 CFEqual(req, kAudioServerPlugInDriverInterfaceUUID);
    CFRelease(req);
    if (!ok) {
        return E_NOINTERFACE;
    }
    pthread_mutex_lock(&gStateMutex);
    ++gPlugInRefCount;
    pthread_mutex_unlock(&gStateMutex);
    *outIface = gDriverRef;
    return S_OK;
}

static ULONG Ivx_AddRef(void* inDriver) {
    if (inDriver != gDriverRef) {
        return 0;
    }
    pthread_mutex_lock(&gStateMutex);
    ULONG c = ++gPlugInRefCount;
    pthread_mutex_unlock(&gStateMutex);
    return c;
}

static ULONG Ivx_Release(void* inDriver) {
    if (inDriver != gDriverRef) {
        return 0;
    }
    pthread_mutex_lock(&gStateMutex);
    if (gPlugInRefCount > 1) {
        --gPlugInRefCount;
    }
    ULONG c = gPlugInRefCount;
    pthread_mutex_unlock(&gStateMutex);
    return c;
}

#pragma mark - Lifecycle

static OSStatus Ivx_Initialize(AudioServerPlugInDriverRef inDriver,
                               AudioServerPlugInHostRef inHost) {
    if (inDriver != gDriverRef) {
        return kAudioHardwareBadObjectError;
    }
    gHost = inHost;
    struct mach_timebase_info tb;
    mach_timebase_info(&tb);
    Float64 hostClockFreq = (Float64)tb.denom / (Float64)tb.numer * 1.0e9;
    gHostTicksPerFrame = hostClockFreq / kSampleRate;
    atomic_fetch_add(&gDiag_InitCalls, 1);

    if (!atomic_load(&gPollerRun)) {
        atomic_store(&gPollerRun, true);
        if (pthread_create(&gPollerThread, NULL, Ring_PollerMain, NULL) != 0) {
            atomic_store(&gPollerRun, false);
            LOG("Initialize: WARNING failed to start ring poller");
        }
    }
    LOG("Initialize: hostTicksPerFrame=%.3f", gHostTicksPerFrame);
    return noErr;
}

// Single static device — creation/destruction at runtime is unsupported.
static OSStatus Ivx_CreateDevice(AudioServerPlugInDriverRef d, CFDictionaryRef a,
                                 const AudioServerPlugInClientInfo* c,
                                 AudioObjectID* o) {
    (void)d; (void)a; (void)c; (void)o;
    return kAudioHardwareUnsupportedOperationError;
}
static OSStatus Ivx_DestroyDevice(AudioServerPlugInDriverRef d, AudioObjectID o) {
    (void)d; (void)o;
    return kAudioHardwareUnsupportedOperationError;
}
static OSStatus Ivx_AddDeviceClient(AudioServerPlugInDriverRef d, AudioObjectID o,
                                    const AudioServerPlugInClientInfo* c) {
    (void)d; (void)o; (void)c;
    return noErr;
}
static OSStatus Ivx_RemoveDeviceClient(AudioServerPlugInDriverRef d, AudioObjectID o,
                                       const AudioServerPlugInClientInfo* c) {
    (void)d; (void)o; (void)c;
    return noErr;
}
static OSStatus Ivx_PerformDeviceConfigurationChange(AudioServerPlugInDriverRef d,
                                                     AudioObjectID o, UInt64 act,
                                                     void* info) {
    (void)d; (void)o; (void)act; (void)info;
    return noErr;
}
static OSStatus Ivx_AbortDeviceConfigurationChange(AudioServerPlugInDriverRef d,
                                                   AudioObjectID o, UInt64 act,
                                                   void* info) {
    (void)d; (void)o; (void)act; (void)info;
    return noErr;
}

#pragma mark - Property helpers

static void FillFormat(AudioStreamBasicDescription* f) {
    f->mSampleRate = gDevice_SampleRate;
    f->mFormatID = kAudioFormatLinearPCM;
    f->mFormatFlags = kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked |
                      kAudioFormatFlagsNativeEndian;
    f->mBytesPerPacket = kBytesPerFrame;
    f->mFramesPerPacket = 1;
    f->mBytesPerFrame = kBytesPerFrame;
    f->mChannelsPerFrame = kChannelsPerFrame;
    f->mBitsPerChannel = kBitsPerChannel;
    f->mReserved = 0;
}

#pragma mark - HasProperty / Settable / Size

static Boolean Ivx_HasProperty(AudioServerPlugInDriverRef inDriver,
                               AudioObjectID inObjectID, pid_t inClientPID,
                               const AudioObjectPropertyAddress* a) {
    (void)inDriver; (void)inClientPID;
    UInt32 sz = 0;
    return Ivx_GetPropertyDataSize(inDriver, inObjectID, inClientPID, a, 0, NULL,
                                   &sz) == noErr;
}

static OSStatus Ivx_IsPropertySettable(AudioServerPlugInDriverRef inDriver,
                                       AudioObjectID inObjectID, pid_t pid,
                                       const AudioObjectPropertyAddress* a,
                                       Boolean* outSettable) {
    (void)inDriver; (void)inObjectID; (void)pid;
    switch (a->mSelector) {
        case kAudioDevicePropertyNominalSampleRate:
        case kAudioStreamPropertyIsActive:
            *outSettable = true;
            break;
        default:
            *outSettable = false;
            break;
    }
    return noErr;
}

static OSStatus Ivx_GetPropertyDataSize(AudioServerPlugInDriverRef inDriver,
                                        AudioObjectID inObjectID, pid_t pid,
                                        const AudioObjectPropertyAddress* a,
                                        UInt32 qsz, const void* q,
                                        UInt32* outSize) {
    (void)inDriver; (void)pid; (void)qsz; (void)q;
    switch (inObjectID) {
        case kObjectID_PlugIn:
            switch (a->mSelector) {
                case kAudioObjectPropertyBaseClass:
                case kAudioObjectPropertyClass:
                case kAudioObjectPropertyOwner:
                case kAudioObjectPropertyManufacturer:
                    *outSize = sizeof(CFStringRef);
                    if (a->mSelector == kAudioObjectPropertyOwner)
                        *outSize = sizeof(AudioObjectID);
                    if (a->mSelector == kAudioObjectPropertyClass ||
                        a->mSelector == kAudioObjectPropertyBaseClass)
                        *outSize = sizeof(AudioClassID);
                    return noErr;
                case kAudioObjectPropertyOwnedObjects:
                case kAudioPlugInPropertyDeviceList:
                    *outSize = sizeof(AudioObjectID);
                    return noErr;
                case kAudioPlugInPropertyTranslateUIDToDevice:
                    *outSize = sizeof(AudioObjectID);
                    return noErr;
                default:
                    return kAudioHardwareUnknownPropertyError;
            }
        case kObjectID_Device:
            switch (a->mSelector) {
                case kAudioObjectPropertyBaseClass:
                case kAudioObjectPropertyClass:
                    *outSize = sizeof(AudioClassID);
                    return noErr;
                case kAudioObjectPropertyOwner:
                    *outSize = sizeof(AudioObjectID);
                    return noErr;
                case kAudioObjectPropertyName:
                case kAudioObjectPropertyManufacturer:
                case kAudioDevicePropertyDeviceUID:
                case kAudioDevicePropertyModelUID:
                    *outSize = sizeof(CFStringRef);
                    return noErr;
                case kAudioObjectPropertyOwnedObjects:
                case kAudioDevicePropertyStreams:
                    *outSize = sizeof(AudioObjectID); // one input stream
                    return noErr;
                case kAudioDevicePropertyDeviceIsRunning:
                case kAudioDevicePropertyDeviceCanBeDefaultDevice:
                case kAudioDevicePropertyDeviceCanBeDefaultSystemDevice:
                case kAudioObjectPropertyControlList: // 0 controls (size 0)
                case kAudioDevicePropertyLatency:
                case kAudioDevicePropertySafetyOffset:
                case kAudioDevicePropertyTransportType:
                case kAudioDevicePropertyClockDomain:
                case kAudioDevicePropertyZeroTimeStampPeriod:
                    *outSize = (a->mSelector == kAudioObjectPropertyControlList)
                                   ? 0
                                   : sizeof(UInt32);
                    return noErr;
                case kAudioDevicePropertyNominalSampleRate:
                    *outSize = sizeof(Float64);
                    return noErr;
                case kAudioDevicePropertyAvailableNominalSampleRates:
                    *outSize = sizeof(AudioValueRange);
                    return noErr;
                case kAudioDevicePropertyStreamConfiguration:
                    *outSize = offsetof(AudioBufferList, mBuffers) +
                               sizeof(AudioBuffer);
                    return noErr;
                case kAudioDevicePropertyIsHidden:
                    *outSize = sizeof(UInt32);
                    return noErr;
                default:
                    return kAudioHardwareUnknownPropertyError;
            }
        case kObjectID_Stream_Input:
            switch (a->mSelector) {
                case kAudioObjectPropertyBaseClass:
                case kAudioObjectPropertyClass:
                    *outSize = sizeof(AudioClassID);
                    return noErr;
                case kAudioObjectPropertyOwner:
                    *outSize = sizeof(AudioObjectID);
                    return noErr;
                case kAudioObjectPropertyOwnedObjects:
                    *outSize = 0;
                    return noErr;
                case kAudioStreamPropertyIsActive:
                case kAudioStreamPropertyDirection:
                case kAudioStreamPropertyTerminalType:
                case kAudioStreamPropertyStartingChannel:
                    *outSize = sizeof(UInt32);
                    return noErr;
                case kAudioStreamPropertyVirtualFormat:
                case kAudioStreamPropertyPhysicalFormat:
                    *outSize = sizeof(AudioStreamBasicDescription);
                    return noErr;
                case kAudioStreamPropertyAvailableVirtualFormats:
                case kAudioStreamPropertyAvailablePhysicalFormats:
                    *outSize = sizeof(AudioStreamRangedDescription);
                    return noErr;
                default:
                    return kAudioHardwareUnknownPropertyError;
            }
        default:
            return kAudioHardwareBadObjectError;
    }
}

#pragma mark - GetPropertyData

static OSStatus Ivx_GetPropertyData(AudioServerPlugInDriverRef inDriver,
                                    AudioObjectID inObjectID, pid_t pid,
                                    const AudioObjectPropertyAddress* a,
                                    UInt32 qsz, const void* q, UInt32 inSize,
                                    UInt32* outSize, void* outData) {
    (void)inDriver; (void)pid; (void)qsz; (void)q;

#define RET_U32(v) do { *((UInt32*)outData) = (v); *outSize = sizeof(UInt32); return noErr; } while (0)
#define RET_OID(v) do { *((AudioObjectID*)outData) = (v); *outSize = sizeof(AudioObjectID); return noErr; } while (0)
#define RET_CLS(v) do { *((AudioClassID*)outData) = (v); *outSize = sizeof(AudioClassID); return noErr; } while (0)
#define RET_STR(s) do { *((CFStringRef*)outData) = CFSTR(s); *outSize = sizeof(CFStringRef); return noErr; } while (0)

    switch (inObjectID) {
        case kObjectID_PlugIn:
            switch (a->mSelector) {
                case kAudioObjectPropertyBaseClass: RET_CLS(kAudioObjectClassID);
                case kAudioObjectPropertyClass:     RET_CLS(kAudioPlugInClassID);
                case kAudioObjectPropertyOwner:     RET_OID(kAudioObjectUnknown);
                case kAudioObjectPropertyManufacturer: RET_STR(kManufacturer_Name);
                case kAudioObjectPropertyOwnedObjects:
                case kAudioPlugInPropertyDeviceList:
                    if (inSize >= sizeof(AudioObjectID)) {
                        ((AudioObjectID*)outData)[0] = kObjectID_Device;
                        *outSize = sizeof(AudioObjectID);
                    } else {
                        *outSize = 0;
                    }
                    return noErr;
                case kAudioPlugInPropertyTranslateUIDToDevice:
                    RET_OID(kObjectID_Device);
                default:
                    return kAudioHardwareUnknownPropertyError;
            }
        case kObjectID_Device:
            switch (a->mSelector) {
                case kAudioObjectPropertyBaseClass: RET_CLS(kAudioObjectClassID);
                case kAudioObjectPropertyClass:     RET_CLS(kAudioDeviceClassID);
                case kAudioObjectPropertyOwner:     RET_OID(kObjectID_PlugIn);
                case kAudioObjectPropertyName:      RET_STR(kDevice_Name);
                case kAudioObjectPropertyManufacturer: RET_STR(kManufacturer_Name);
                case kAudioDevicePropertyDeviceUID: RET_STR(kDevice_UID);
                case kAudioDevicePropertyModelUID:  RET_STR(kDevice_ModelUID);
                case kAudioDevicePropertyTransportType:
                    RET_U32(kAudioDeviceTransportTypeVirtual);
                case kAudioDevicePropertyClockDomain: RET_U32(0);
                case kAudioDevicePropertyDeviceIsRunning:
                    RET_U32(gDevice_IORunningClients > 0 ? 1 : 0);
                case kAudioDevicePropertyDeviceCanBeDefaultDevice:
                case kAudioDevicePropertyDeviceCanBeDefaultSystemDevice:
                    RET_U32(1);
                case kAudioDevicePropertyLatency:        RET_U32(0);
                case kAudioDevicePropertySafetyOffset:   RET_U32(0);
                case kAudioDevicePropertyZeroTimeStampPeriod:
                    RET_U32(kRingPeriodFrames);
                case kAudioDevicePropertyIsHidden:       RET_U32(0);
                case kAudioObjectPropertyControlList:    *outSize = 0; return noErr;
                case kAudioDevicePropertyNominalSampleRate:
                    *((Float64*)outData) = gDevice_SampleRate;
                    *outSize = sizeof(Float64);
                    return noErr;
                case kAudioDevicePropertyAvailableNominalSampleRates: {
                    AudioValueRange* r = (AudioValueRange*)outData;
                    r->mMinimum = kSampleRate;
                    r->mMaximum = kSampleRate;
                    *outSize = sizeof(AudioValueRange);
                    return noErr;
                }
                case kAudioObjectPropertyOwnedObjects:
                case kAudioDevicePropertyStreams:
                    if (inSize >= sizeof(AudioObjectID)) {
                        ((AudioObjectID*)outData)[0] = kObjectID_Stream_Input;
                        *outSize = sizeof(AudioObjectID);
                    } else {
                        *outSize = 0;
                    }
                    return noErr;
                case kAudioDevicePropertyStreamConfiguration: {
                    AudioBufferList* bl = (AudioBufferList*)outData;
                    bl->mNumberBuffers = 1;
                    bl->mBuffers[0].mNumberChannels = kChannelsPerFrame;
                    bl->mBuffers[0].mDataByteSize = 0;
                    bl->mBuffers[0].mData = NULL;
                    *outSize = offsetof(AudioBufferList, mBuffers) +
                               sizeof(AudioBuffer);
                    return noErr;
                }
                default:
                    return kAudioHardwareUnknownPropertyError;
            }
        case kObjectID_Stream_Input:
            switch (a->mSelector) {
                case kAudioObjectPropertyBaseClass: RET_CLS(kAudioObjectClassID);
                case kAudioObjectPropertyClass:     RET_CLS(kAudioStreamClassID);
                case kAudioObjectPropertyOwner:     RET_OID(kObjectID_Device);
                case kAudioObjectPropertyOwnedObjects: *outSize = 0; return noErr;
                case kAudioStreamPropertyDirection:  RET_U32(1); // input
                case kAudioStreamPropertyTerminalType:
                    RET_U32(kAudioStreamTerminalTypeMicrophone);
                case kAudioStreamPropertyStartingChannel: RET_U32(1);
                case kAudioStreamPropertyIsActive:
                    RET_U32(gStream_Active ? 1 : 0);
                case kAudioStreamPropertyVirtualFormat:
                case kAudioStreamPropertyPhysicalFormat:
                    FillFormat((AudioStreamBasicDescription*)outData);
                    *outSize = sizeof(AudioStreamBasicDescription);
                    return noErr;
                case kAudioStreamPropertyAvailableVirtualFormats:
                case kAudioStreamPropertyAvailablePhysicalFormats: {
                    AudioStreamRangedDescription* rd =
                        (AudioStreamRangedDescription*)outData;
                    FillFormat(&rd->mFormat);
                    rd->mSampleRateRange.mMinimum = kSampleRate;
                    rd->mSampleRateRange.mMaximum = kSampleRate;
                    *outSize = sizeof(AudioStreamRangedDescription);
                    return noErr;
                }
                default:
                    return kAudioHardwareUnknownPropertyError;
            }
        default:
            return kAudioHardwareBadObjectError;
    }
}

static OSStatus Ivx_SetPropertyData(AudioServerPlugInDriverRef inDriver,
                                    AudioObjectID inObjectID, pid_t pid,
                                    const AudioObjectPropertyAddress* a,
                                    UInt32 qsz, const void* q, UInt32 inSize,
                                    const void* inData) {
    (void)inDriver; (void)pid; (void)qsz; (void)q; (void)inSize;
    if (inObjectID == kObjectID_Stream_Input &&
        a->mSelector == kAudioStreamPropertyIsActive) {
        gStream_Active = (*((const UInt32*)inData) != 0);
        return noErr;
    }
    if (inObjectID == kObjectID_Device &&
        a->mSelector == kAudioDevicePropertyNominalSampleRate) {
        // Only 48 kHz is offered; accept the no-op set.
        return noErr;
    }
    return kAudioHardwareUnknownPropertyError;
}

#pragma mark - IO

static OSStatus Ivx_StartIO(AudioServerPlugInDriverRef inDriver,
                            AudioObjectID inDeviceObjectID, UInt32 inClientID) {
    (void)inDriver; (void)inDeviceObjectID; (void)inClientID;
    pthread_mutex_lock(&gStateMutex);
    atomic_fetch_add(&gDiag_StartIO, 1);
    if (gDevice_IORunningClients == 0) {
        gAnchorHostTime = mach_absolute_time();
        gNumberTimeStamps = 0;
        atomic_store(&gIOActive, true); // poller will (re)map within ~250 ms
        LOG("StartIO: device active, poller acquiring ring");
    }
    ++gDevice_IORunningClients;
    pthread_mutex_unlock(&gStateMutex);
    return noErr;
}

static OSStatus Ivx_StopIO(AudioServerPlugInDriverRef inDriver,
                           AudioObjectID inDeviceObjectID, UInt32 inClientID) {
    (void)inDriver; (void)inDeviceObjectID; (void)inClientID;
    atomic_fetch_add(&gDiag_StopIO, 1);
    pthread_mutex_lock(&gStateMutex);
    if (gDevice_IORunningClients > 0) {
        --gDevice_IORunningClients;
    }
    if (gDevice_IORunningClients == 0) {
        atomic_store(&gIOActive, false); // poller retires the mapping
        LOG("StopIO: device idle");
    }
    pthread_mutex_unlock(&gStateMutex);
    return noErr;
}

static OSStatus Ivx_GetZeroTimeStamp(AudioServerPlugInDriverRef inDriver,
                                     AudioObjectID inDeviceObjectID,
                                     UInt32 inClientID, Float64* outSampleTime,
                                     UInt64* outHostTime, UInt64* outSeed) {
    (void)inDriver; (void)inDeviceObjectID; (void)inClientID;
    UInt64 now = mach_absolute_time();
    Float64 ticksPerPeriod = gHostTicksPerFrame * (Float64)kRingPeriodFrames;
    UInt64 elapsed = now - gAnchorHostTime;
    UInt64 periods = (UInt64)((Float64)elapsed / ticksPerPeriod);
    if (periods > gNumberTimeStamps) {
        gNumberTimeStamps = periods;
    }
    *outSampleTime = (Float64)(gNumberTimeStamps * kRingPeriodFrames);
    *outHostTime =
        gAnchorHostTime + (UInt64)((Float64)gNumberTimeStamps * ticksPerPeriod);
    *outSeed = 1;
    return noErr;
}

static OSStatus Ivx_WillDoIOOperation(AudioServerPlugInDriverRef inDriver,
                                      AudioObjectID inDeviceObjectID,
                                      UInt32 inClientID, UInt32 inOperationID,
                                      Boolean* outWillDo,
                                      Boolean* outWillDoInPlace) {
    (void)inDriver; (void)inDeviceObjectID; (void)inClientID;
    Boolean will = (inOperationID == kAudioServerPlugInIOOperationReadInput);
    if (outWillDo) *outWillDo = will;
    if (outWillDoInPlace) *outWillDoInPlace = true;
    return noErr;
}

static OSStatus Ivx_BeginIOOperation(AudioServerPlugInDriverRef d, AudioObjectID o,
                                     UInt32 c, UInt32 op, UInt32 n,
                                     const AudioServerPlugInIOCycleInfo* i) {
    (void)d; (void)o; (void)c; (void)op; (void)n; (void)i;
    return noErr;
}

// Realtime path. Copies mono Float32 from the shared ring buffer into the
// host's input buffer; silence on underrun / absent producer. No locks, no
// allocation, no syscalls (rules §19.4, §19.5).
static OSStatus Ivx_DoIOOperation(AudioServerPlugInDriverRef inDriver,
                                  AudioObjectID inDeviceObjectID,
                                  AudioObjectID inStreamObjectID,
                                  UInt32 inClientID, UInt32 inOperationID,
                                  UInt32 inIOBufferFrameSize,
                                  const AudioServerPlugInIOCycleInfo* inIOCycleInfo,
                                  void* ioMainBuffer, void* ioSecondaryBuffer) {
    (void)inDriver; (void)inDeviceObjectID; (void)inStreamObjectID;
    (void)inClientID; (void)inIOCycleInfo; (void)ioSecondaryBuffer;
    if (inOperationID != kAudioServerPlugInIOOperationReadInput ||
        ioMainBuffer == NULL) {
        return noErr;
    }
    intervox_ring_t* rb =
        atomic_load_explicit(&gRing, memory_order_acquire);
    atomic_fetch_add_explicit(&gDiag_DoIO, 1, memory_order_relaxed);
    atomic_store_explicit(&gDiag_LastFrames, (int)inIOBufferFrameSize,
                          memory_order_relaxed);
    if (rb != NULL) {
        atomic_fetch_add_explicit(&gDiag_DoIOWithRing, 1,
                                  memory_order_relaxed);
    }
    intervox_ring_read(rb, (float*)ioMainBuffer, inIOBufferFrameSize);
    return noErr;
}

static OSStatus Ivx_EndIOOperation(AudioServerPlugInDriverRef d, AudioObjectID o,
                                   UInt32 c, UInt32 op, UInt32 n,
                                   const AudioServerPlugInIOCycleInfo* i) {
    (void)d; (void)o; (void)c; (void)op; (void)n; (void)i;
    return noErr;
}
