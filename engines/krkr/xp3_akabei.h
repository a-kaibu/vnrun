// Akabei-style XP3 encryption: a 32-byte rolling XOR key derived from the file hash and seed.
// Offsets are in the uncompressed file, so seeks and segment boundaries keep the same key.
#pragma once

#include <cstddef>
#include <cstdint>

namespace vnrun {
inline void AkabeiDecrypt(std::uint32_t seed, std::uint32_t hash, std::uint64_t offset,
                          std::uint8_t *bytes, std::size_t size, unsigned period = 32)
{
    std::uint32_t state = (hash ^ seed) & 0x7fffffff;
    state |= state << 31;
    std::uint8_t key[32];
    for (auto &byte : key) {
        byte = static_cast<std::uint8_t>(state);
        state = ((state & 0xfffffffe) << 23) | (state >> 8);
    }
    for (std::size_t i = 0; i < size; ++i)
        bytes[i] ^= key[((offset % period) + (i % period)) % period];
}
}
