#include <stdio.h>
#include <stdlib.h>

#include "../src/Intervox.c"

static void Check(bool ok, const char* expression, const char* file, int line) {
    if (!ok) {
        fprintf(stderr, "%s:%d: check failed: %s\n", file, line, expression);
        exit(1);
    }
}

#define CHECK(expr) Check((expr), #expr, __FILE__, __LINE__)

static AudioObjectPropertyAddress Addr(AudioObjectPropertySelector selector,
                                       AudioObjectPropertyScope scope,
                                       AudioObjectPropertyElement element) {
    AudioObjectPropertyAddress a = { selector, scope, element };
    return a;
}

static UInt32 SizeOf(AudioObjectID object,
                     const AudioObjectPropertyAddress* address,
                     UInt32 qualifier_size,
                     const void* qualifier) {
    UInt32 size = 0xFFFFFFFFu;
    OSStatus err = Ivx_GetPropertyDataSize(gDriverRef, object, 0, address,
                                           qualifier_size, qualifier, &size);
    CHECK(err == noErr);
    return size;
}

static void ExpectUnknown(AudioObjectID object,
                          const AudioObjectPropertyAddress* address) {
    UInt32 size = 0;
    OSStatus err = Ivx_GetPropertyDataSize(gDriverRef, object, 0, address,
                                           0, NULL, &size);
    CHECK(err == kAudioHardwareUnknownPropertyError);
}

static UInt32 GetUInt32(AudioObjectID object,
                        const AudioObjectPropertyAddress* address) {
    UInt32 value = 0xFFFFFFFFu;
    UInt32 size = 0;
    OSStatus err = Ivx_GetPropertyData(gDriverRef, object, 0, address,
                                       0, NULL, sizeof(value), &size, &value);
    CHECK(err == noErr);
    CHECK(size == sizeof(value));
    return value;
}

static AudioObjectID GetObjectID(AudioObjectID object,
                                 const AudioObjectPropertyAddress* address) {
    AudioObjectID value = kAudioObjectUnknown;
    UInt32 size = 0;
    OSStatus err = Ivx_GetPropertyData(gDriverRef, object, 0, address,
                                       0, NULL, sizeof(value), &size, &value);
    CHECK(err == noErr);
    CHECK(size == sizeof(value));
    return value;
}

static void ExpectObjectListSize(AudioObjectID object,
                                 const AudioObjectPropertyAddress* address,
                                 UInt32 qualifier_size,
                                 const void* qualifier,
                                 UInt32 expected_count) {
    UInt32 expected_size = expected_count * (UInt32)sizeof(AudioObjectID);
    CHECK(SizeOf(object, address, qualifier_size, qualifier) == expected_size);

    AudioObjectID ids[2] = { 0, 0 };
    UInt32 size = 0xFFFFFFFFu;
    OSStatus err = Ivx_GetPropertyData(gDriverRef, object, 0, address,
                                       qualifier_size, qualifier, sizeof(ids),
                                       &size, ids);
    CHECK(err == noErr);
    CHECK(size == expected_size);
}

static void TestOwnedObjectsRespectQualifiers(void) {
    AudioObjectPropertyAddress owned = Addr(kAudioObjectPropertyOwnedObjects,
                                            kAudioObjectPropertyScopeGlobal,
                                            kAudioObjectPropertyElementMain);
    AudioClassID object_filter = kAudioObjectClassID;
    AudioClassID device_filter = kAudioDeviceClassID;
    AudioClassID stream_filter = kAudioStreamClassID;
    AudioClassID control_filter = kAudioControlClassID;

    ExpectObjectListSize(kObjectID_PlugIn, &owned, 0, NULL, 1);
    ExpectObjectListSize(kObjectID_PlugIn, &owned, sizeof(object_filter),
                         &object_filter, 1);
    ExpectObjectListSize(kObjectID_PlugIn, &owned, sizeof(device_filter),
                         &device_filter, 1);
    ExpectObjectListSize(kObjectID_PlugIn, &owned, sizeof(stream_filter),
                         &stream_filter, 0);
    ExpectObjectListSize(kObjectID_PlugIn, &owned, sizeof(control_filter),
                         &control_filter, 0);

    ExpectObjectListSize(kObjectID_Device, &owned, 0, NULL, 1);
    ExpectObjectListSize(kObjectID_Device, &owned, sizeof(object_filter),
                         &object_filter, 1);
    ExpectObjectListSize(kObjectID_Device, &owned, sizeof(stream_filter),
                         &stream_filter, 1);
    ExpectObjectListSize(kObjectID_Device, &owned, sizeof(control_filter),
                         &control_filter, 0);
}

static void TestInputOnlyPublication(void) {
    AudioObjectPropertyAddress streams_global = Addr(kAudioDevicePropertyStreams,
                                                     kAudioObjectPropertyScopeGlobal,
                                                     kAudioObjectPropertyElementMain);
    AudioObjectPropertyAddress streams_input = Addr(kAudioDevicePropertyStreams,
                                                    kAudioObjectPropertyScopeInput,
                                                    kAudioObjectPropertyElementMain);
    AudioObjectPropertyAddress streams_output = Addr(kAudioDevicePropertyStreams,
                                                     kAudioObjectPropertyScopeOutput,
                                                     kAudioObjectPropertyElementMain);

    CHECK(SizeOf(kObjectID_Device, &streams_global, 0, NULL) ==
           sizeof(AudioObjectID));
    CHECK(SizeOf(kObjectID_Device, &streams_input, 0, NULL) ==
           sizeof(AudioObjectID));
    CHECK(SizeOf(kObjectID_Device, &streams_output, 0, NULL) == 0);

    AudioObjectPropertyAddress cfg_input =
        Addr(kAudioDevicePropertyStreamConfiguration,
             kAudioObjectPropertyScopeInput,
             kAudioObjectPropertyElementMain);
    AudioObjectPropertyAddress cfg_output =
        Addr(kAudioDevicePropertyStreamConfiguration,
             kAudioObjectPropertyScopeOutput,
             kAudioObjectPropertyElementMain);
    CHECK(SizeOf(kObjectID_Device, &cfg_input, 0, NULL) ==
           offsetof(AudioBufferList, mBuffers) + sizeof(AudioBuffer));
    CHECK(SizeOf(kObjectID_Device, &cfg_output, 0, NULL) ==
           offsetof(AudioBufferList, mBuffers));

    AudioObjectPropertyAddress default_global =
        Addr(kAudioDevicePropertyDeviceCanBeDefaultDevice,
             kAudioObjectPropertyScopeGlobal,
             kAudioObjectPropertyElementMain);
    AudioObjectPropertyAddress default_input =
        Addr(kAudioDevicePropertyDeviceCanBeDefaultDevice,
             kAudioObjectPropertyScopeInput,
             kAudioObjectPropertyElementMain);
    AudioObjectPropertyAddress default_output =
        Addr(kAudioDevicePropertyDeviceCanBeDefaultDevice,
             kAudioObjectPropertyScopeOutput,
             kAudioObjectPropertyElementMain);
    CHECK(GetUInt32(kObjectID_Device, &default_global) == 0);
    CHECK(GetUInt32(kObjectID_Device, &default_input) == 1);
    CHECK(GetUInt32(kObjectID_Device, &default_output) == 0);
}

static void TestStableCoreProperties(void) {
    AudioObjectPropertyAddress related = Addr(kAudioDevicePropertyRelatedDevices,
                                              kAudioObjectPropertyScopeGlobal,
                                              kAudioObjectPropertyElementMain);
    CHECK(SizeOf(kObjectID_Device, &related, 0, NULL) ==
           sizeof(AudioObjectID));
    CHECK(GetObjectID(kObjectID_Device, &related) == kObjectID_Device);

    AudioObjectPropertyAddress stream_name = Addr(kAudioObjectPropertyName,
                                                  kAudioObjectPropertyScopeGlobal,
                                                  kAudioObjectPropertyElementMain);
    CHECK(SizeOf(kObjectID_Stream_Input, &stream_name, 0, NULL) ==
           sizeof(CFStringRef));

    AudioObjectPropertyAddress stream_latency =
        Addr(kAudioStreamPropertyLatency,
             kAudioObjectPropertyScopeGlobal,
             kAudioObjectPropertyElementMain);
    CHECK(GetUInt32(kObjectID_Stream_Input, &stream_latency) == 0);
}

static void TestInvalidAddressAndBuffers(void) {
    AudioObjectPropertyAddress bad_element = Addr(kAudioDevicePropertyDeviceUID,
                                                  kAudioObjectPropertyScopeGlobal,
                                                  1);
    ExpectUnknown(kObjectID_Device, &bad_element);

    AudioObjectPropertyAddress uid = Addr(kAudioDevicePropertyDeviceUID,
                                          kAudioObjectPropertyScopeGlobal,
                                          kAudioObjectPropertyElementMain);
    UInt8 tiny = 0;
    UInt32 size = 0xFFFFFFFFu;
    OSStatus err = Ivx_GetPropertyData(gDriverRef, kObjectID_Device, 0, &uid,
                                       0, NULL, sizeof(tiny), &size, &tiny);
    CHECK(err == kAudioHardwareBadPropertySizeError);
    CHECK(size == 0);
}

static void TestSettableContract(void) {
    AudioObjectPropertyAddress nominal = Addr(kAudioDevicePropertyNominalSampleRate,
                                              kAudioObjectPropertyScopeGlobal,
                                              kAudioObjectPropertyElementMain);
    Boolean settable = true;
    OSStatus err = Ivx_IsPropertySettable(gDriverRef, kObjectID_Device, 0,
                                          &nominal, &settable);
    CHECK(err == noErr);
    CHECK(!settable);

    Float64 rate = kSampleRate;
    err = Ivx_SetPropertyData(gDriverRef, kObjectID_Device, 0, &nominal,
                              0, NULL, sizeof(rate), &rate);
    CHECK(err == kAudioHardwareIllegalOperationError);

    AudioObjectPropertyAddress active = Addr(kAudioStreamPropertyIsActive,
                                             kAudioObjectPropertyScopeGlobal,
                                             kAudioObjectPropertyElementMain);
    settable = false;
    err = Ivx_IsPropertySettable(gDriverRef, kObjectID_Stream_Input, 0,
                                 &active, &settable);
    CHECK(err == noErr);
    CHECK(settable);

    UInt32 disabled = 0;
    err = Ivx_SetPropertyData(gDriverRef, kObjectID_Stream_Input, 0, &active,
                              0, NULL, sizeof(disabled), &disabled);
    CHECK(err == noErr);
    CHECK(GetUInt32(kObjectID_Stream_Input, &active) == 0);
}

static void TestIOEntrypointValidation(void) {
    CHECK(Ivx_StartIO(NULL, kObjectID_Device, 0) == kAudioHardwareBadObjectError);
    CHECK(Ivx_StartIO(gDriverRef, kObjectID_Stream_Input, 0) ==
          kAudioHardwareBadObjectError);

    CHECK(Ivx_StartIO(gDriverRef, kObjectID_Device, 42) == noErr);
    gHostTicksPerFrame = 1.0;

    AudioObjectPropertyAddress running = Addr(kAudioDevicePropertyDeviceIsRunning,
                                              kAudioObjectPropertyScopeGlobal,
                                              kAudioObjectPropertyElementMain);
    CHECK(GetUInt32(kObjectID_Device, &running) == 1);

    Float64 sample_time = -1.0;
    UInt64 host_time = 0;
    UInt64 seed = 0;
    CHECK(Ivx_GetZeroTimeStamp(gDriverRef, kObjectID_Device, 42, &sample_time,
                               &host_time, &seed) == noErr);
    CHECK(sample_time >= 0.0);
    CHECK(host_time != 0);
    CHECK(seed == 1);
    CHECK(Ivx_GetZeroTimeStamp(gDriverRef, kObjectID_Device, 42, NULL,
                               &host_time, &seed) ==
          kAudioHardwareIllegalOperationError);

    Boolean will = true;
    Boolean in_place = true;
    CHECK(Ivx_WillDoIOOperation(gDriverRef, kObjectID_Device, 42, 0, &will,
                                &in_place) == noErr);
    CHECK(!will);
    CHECK(!in_place);
    CHECK(Ivx_WillDoIOOperation(gDriverRef, kObjectID_Device, 42,
                                kAudioServerPlugInIOOperationReadInput, NULL,
                                &in_place) ==
          kAudioHardwareIllegalOperationError);

    float out[16] = { 1.0f };
    CHECK(Ivx_DoIOOperation(gDriverRef, kObjectID_Device, kObjectID_PlugIn, 42,
                            kAudioServerPlugInIOOperationReadInput, 16, NULL,
                            out, NULL) == kAudioHardwareBadObjectError);
    CHECK(Ivx_DoIOOperation(gDriverRef, kObjectID_Device, kObjectID_Stream_Input,
                            42, kAudioServerPlugInIOOperationReadInput, 16,
                            NULL, NULL, NULL) ==
          kAudioHardwareIllegalOperationError);

    CHECK(Ivx_StopIO(gDriverRef, kObjectID_Device, 42) == noErr);
    CHECK(GetUInt32(kObjectID_Device, &running) == 0);
}

static void StoreSample(intervox_ring_t* rb, uint64_t index, float sample) {
    uint32_t bits = 0;
    memcpy(&bits, &sample, sizeof(bits));
    atomic_store_explicit(&rb->frames[index % INTERVOX_RING_CAPACITY], bits,
                          memory_order_relaxed);
}

static void TestRingReadDropsStaleBacklog(void) {
    intervox_ring_t* rb = calloc(1, sizeof(intervox_ring_t));
    CHECK(rb != NULL);
    rb->magic = INTERVOX_RING_MAGIC;
    rb->version = INTERVOX_RING_VERSION;
    rb->sample_rate = 48000u;
    rb->channels = 1u;
    rb->capacity_frames = INTERVOX_RING_CAPACITY;

    const uint64_t w = 20000u;
    const uint64_t live_start = w - INTERVOX_RING_LIVE_MAX_UNREAD;
    atomic_store_explicit(&rb->write_index, w, memory_order_relaxed);
    atomic_store_explicit(&rb->read_index, 0u, memory_order_relaxed);
    StoreSample(rb, 0u, 99.0f);
    StoreSample(rb, live_start, 2.0f);
    StoreSample(rb, live_start + 1u, 3.0f);
    StoreSample(rb, live_start + 2u, 4.0f);
    StoreSample(rb, live_start + 3u, 5.0f);

    float out[4] = { 0 };
    CHECK(!intervox_ring_read(rb, out, 4));
    CHECK(out[0] == 2.0f);
    CHECK(out[1] == 3.0f);
    CHECK(out[2] == 4.0f);
    CHECK(out[3] == 5.0f);
    CHECK(atomic_load_explicit(&rb->read_index, memory_order_relaxed) ==
          live_start + 4u);
    free(rb);
}

int main(void) {
    TestOwnedObjectsRespectQualifiers();
    TestInputOnlyPublication();
    TestStableCoreProperties();
    TestInvalidAddressAndBuffers();
    TestSettableContract();
    TestIOEntrypointValidation();
    TestRingReadDropsStaleBacklog();
    puts("driver property contract ok");
    return 0;
}
