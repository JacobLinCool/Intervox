// Shared audio ring buffer — C view of the Rust `SharedAudioRingBuffer`
// (#[repr(C)]) in crates/intervox-core/src/virtual_mic/ring_buffer.rs.
//
// THE FIELD ORDER, TYPES AND SIZES BELOW MUST STAY BYTE-IDENTICAL to the Rust
// definition. A _Static_assert pins the total size; the Rust side asserts the
// same number (1_536_056). The HAL driver is the CONSUMER and runs inside
// coreaudiod on a realtime thread: the read path must never lock, allocate, or
// make syscalls.

#ifndef INTERVOX_RING_H
#define INTERVOX_RING_H

#include <stdatomic.h>
#include <stdbool.h>
#include <stdint.h>
#include <fcntl.h>
#include <sys/mman.h>
#include <string.h>
#include <unistd.h>

#define INTERVOX_RING_CAPACITY 384000u   /* 48 kHz * 8 s */
/* The capacity is a safety window, not live FIFO latency.  Keep at most
 * 100 ms unread on the realtime read path; older samples are stale. */
#define INTERVOX_RING_LIVE_MAX_UNREAD 4800u
#define INTERVOX_RING_MAGIC    0x49564F58u /* "IVOX" */
#define INTERVOX_RING_VERSION  2u
#define INTERVOX_SHM_NAME      "/intervox.ring"

/* Mode values mirror Rust VirtualMicMode discriminants (informational; the
 * driver only ever reads/plays whatever the producer wrote). */
typedef enum {
    INTERVOX_MODE_SILENCE = 0,
    INTERVOX_MODE_PASS_THROUGH = 1,
    INTERVOX_MODE_TRANSLATE = 2,
    INTERVOX_MODE_TRANSLATE_WITH_ORIGINAL = 3,
} intervox_mode_t;

typedef struct {
    _Atomic uint64_t sequence;
    _Atomic uint32_t bits;
    uint32_t _pad;
} intervox_ring_slot_t;

/* Natural alignment already matches Rust #[repr(C)]: max field align is 8
 * (uint64_t), header is 56 bytes (mult. of 8), total is mult. of 8 — no
 * implicit padding. The _Static_assert below pins this. */
typedef struct {
    uint32_t magic;            /* off 0  */
    uint32_t version;          /* off 4  */
    uint32_t sample_rate;      /* off 8  */
    uint32_t channels;         /* off 12 */
    uint64_t capacity_frames;  /* off 16 */
    _Atomic uint64_t write_index; /* off 24 */
    _Atomic uint64_t read_index;  /* off 32 */
    _Atomic uint64_t generation;  /* off 40 */
    _Atomic uint32_t mode;        /* off 48 */
    uint32_t _pad;                /* off 52 */
    intervox_ring_slot_t slots[INTERVOX_RING_CAPACITY]; /* off 56 */
} intervox_ring_t;

_Static_assert(sizeof(intervox_ring_slot_t) == 16u,
               "intervox_ring_slot_t layout must match Rust RingSlot");
_Static_assert(sizeof(intervox_ring_t) == 56u + INTERVOX_RING_CAPACITY * 16u,
               "intervox_ring_t layout must match Rust SharedAudioRingBuffer");

static inline void intervox_ring_close(intervox_ring_t* rb, int fd) {
    if (rb) {
        munmap((void*)rb, sizeof(intervox_ring_t));
    }
    if (fd >= 0) {
        close(fd);
    }
}

static inline void intervox_ring_advance_read_index_to(intervox_ring_t* rb,
                                                       uint64_t target) {
    uint64_t current =
        atomic_load_explicit(&rb->read_index, memory_order_acquire);
    while (target > current &&
           !atomic_compare_exchange_weak_explicit(
               &rb->read_index, &current, target, memory_order_release,
               memory_order_acquire)) {
    }
}

/* Realtime-safe consumer read. Fills `out` entirely; missing samples become
 * silence. Returns true on underrun. No locks, no allocation, no syscalls. */
static inline bool intervox_ring_read(intervox_ring_t* rb, float* out,
                                      uint32_t n) {
    if (rb == NULL) {
        memset(out, 0, (size_t)n * sizeof(float));
        return true;
    }
    const uint64_t cap = INTERVOX_RING_CAPACITY;
    uint64_t r = atomic_load_explicit(&rb->read_index, memory_order_relaxed);
    uint64_t w = atomic_load_explicit(&rb->write_index, memory_order_acquire);
    uint64_t avail = w - r;
    if (avail > cap) {
        intervox_ring_advance_read_index_to(rb, w);
        r = w;
        avail = 0;
    } else if (avail > INTERVOX_RING_LIVE_MAX_UNREAD) {
        r = w - INTERVOX_RING_LIVE_MAX_UNREAD;
        intervox_ring_advance_read_index_to(rb, r);
        avail = INTERVOX_RING_LIVE_MAX_UNREAD;
    }
    uint64_t take = (avail < n) ? avail : n;
    for (uint32_t i = 0; i < n; ++i) {
        if (i < take) {
            const uint64_t index = r + i;
            const uint64_t expected = index << 1;
            intervox_ring_slot_t* slot = &rb->slots[index % cap];
            uint64_t before =
                atomic_load_explicit(&slot->sequence, memory_order_acquire);
            if (before == expected) {
                uint32_t bits = atomic_load_explicit(&slot->bits,
                                                     memory_order_relaxed);
                uint64_t after =
                    atomic_load_explicit(&slot->sequence, memory_order_acquire);
                if (after == before) {
                    float sample = 0.0f;
                    memcpy(&sample, &bits, sizeof(sample));
                    out[i] = sample;
                } else {
                    out[i] = 0.0f;
                }
            } else {
                out[i] = 0.0f;
            }
        } else {
            out[i] = 0.0f;
        }
    }
    intervox_ring_advance_read_index_to(rb, r + take);
    return take < n;
}

#endif /* INTERVOX_RING_H */
