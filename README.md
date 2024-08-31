### `tscns` is a Time Stamp Counter clock source
+ `tscns` uses a global static variable to cache the clock counter and system clock reference difference. `tscns` is thread-safe.
+ `tscns` globally uses the atomic `AtomicUsize` type and aligns it to the CPU cache line, ensuring performance during multi-threaded access.
+ CPU cache line alignment relies on `[crossbeam_utils::CachePadded]`, and `CachePadded` is suitable for most platforms.

#### Supported Platforms

- **x86_64**
  + On x86_64, the value of the `Counter` register is obtained via the `rdtsc` instruction, and the clock source frequency is calculated through a random sampling rate. The frequency is aligned with the system clock source.
- **arm64**
  + On arm64, the value of the `cntvct_el0` register is obtained via the inline assembly instruction `mrs`, and the clock source frequency is calculated through a random sampling rate. The frequency is aligned with the system clock source.
- **mips64**
  + On mips64, the value of the `reg` register is obtained via the inline assembly instruction `rdtime`, and the clock source frequency is calculated through a random sampling rate. The frequency is aligned with the system clock source.
