// Standalone allocation/timing diagnostic, never linked into the application.
// Global new/new[] (including aligned forms) are counted. Direct C malloc/calloc/realloc
// inside dependencies are not intercepted; these numbers are not a total-heap claim.
#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <new>

static thread_local bool measuring = false;
static thread_local unsigned long long allocations = 0, bytes = 0;
static void record(std::size_t n) { if (measuring) { ++allocations; bytes += n; } }
void* operator new(std::size_t n) { record(n); if (void* p = std::malloc(n ? n : 1)) return p; throw std::bad_alloc(); }
void* operator new[](std::size_t n) { return ::operator new(n); }
void operator delete(void* p) noexcept { std::free(p); }
void operator delete[](void* p) noexcept { std::free(p); }
void operator delete(void* p, std::size_t) noexcept { std::free(p); }
void operator delete[](void* p, std::size_t) noexcept { std::free(p); }
void* operator new(std::size_t n, std::align_val_t a) {
  record(n); void* p = nullptr;
  if (posix_memalign(&p, static_cast<std::size_t>(a), n ? n : 1) != 0) throw std::bad_alloc();
  return p;
}
void* operator new[](std::size_t n, std::align_val_t a) { return ::operator new(n, a); }
void operator delete(void* p, std::align_val_t) noexcept { std::free(p); }
void operator delete[](void* p, std::align_val_t) noexcept { std::free(p); }
void operator delete(void* p, std::size_t, std::align_val_t) noexcept { std::free(p); }
void operator delete[](void* p, std::size_t, std::align_val_t) noexcept { std::free(p); }

extern "C" {
void* openkrisp_aec_create(int) noexcept;
void openkrisp_aec_destroy(void*) noexcept;
int openkrisp_aec_render(void*, const float*, const float*) noexcept;
int openkrisp_aec_capture(void*, const float*, float*) noexcept;
}
using Clock = std::chrono::steady_clock;
static double us(Clock::time_point start) { return std::chrono::duration<double, std::micro>(Clock::now() - start).count(); }
int main() {
  std::array<float, 480> left{}, right{}, capture{}, output{};
  std::array<double, 1000> timing{};
  measuring = true;
  auto start = Clock::now();
  void* state = openkrisp_aec_create(2);
  const auto construction_us = us(start);
  measuring = false;
  if (!state) return 1;
  std::printf("construct_new_calls=%llu construct_requested_bytes=%llu construct_us=%.2f\n", allocations, bytes, construction_us);
  allocations = 0; bytes = 0;
  unsigned misses = 0;
  for (unsigned frame = 0; frame < 1100; ++frame) {
    for (unsigned i = 0; i < 480; ++i) {
      const double t = (frame * 480 + i) / 48000.0;
      left[i] = 0.1f * std::sin(t * 2900); right[i] = 0.1f * std::cos(t * 1700);
      capture[i] = 0.05f * std::sin((t - 0.1) * 2900) + 0.1f * std::sin(t * 4100);
    }
    measuring = frame >= 100;
    start = Clock::now();
    const int r = openkrisp_aec_render(state, left.data(), right.data());
    const int c = openkrisp_aec_capture(state, capture.data(), output.data());
    const double elapsed = us(start);
    measuring = false;
    if (r || c) return 2;
    if (frame >= 100) { timing[frame - 100] = elapsed; misses += elapsed > 10000; }
    for (float sample : output) if (!std::isfinite(sample)) return 3;
  }
  std::sort(timing.begin(), timing.end());
  std::printf("frames=1000 steady_new_calls=%llu steady_requested_bytes=%llu p50_us=%.2f p95_us=%.2f p99_us=%.2f max_us=%.2f missed_deadlines=%u\n", allocations, bytes, timing[499], timing[949], timing[989], timing[999], misses);
  start = Clock::now();
  openkrisp_aec_destroy(state);
  state = openkrisp_aec_create(2);
  std::printf("reconstruction_us=%.2f\n", us(start));
  if (!state) return 4;
  openkrisp_aec_destroy(state);
  return 0;
}
