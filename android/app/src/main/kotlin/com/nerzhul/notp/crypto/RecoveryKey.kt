package com.nerzhul.notp.crypto

import java.util.zip.CRC32

/**
 * Recovery key encoding for the v3 envelope.
 *
 * Format: `V3-<base32 groups of 5 separated by `-`>-<8-hex CRC32>`.
 *
 * The version prefix is appended to the front so a future envelope can ship
 * a different recovery key shape without collision. The CRC32 covers the raw
 * 32-byte key.
 */
object RecoveryKey {

    private const val PREFIX = "V3"
    private const val GROUP_SIZE = 5
    private val ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567".toCharArray()

    fun format(bytes: ByteArray): String {
        require(bytes.size == 32) { "Recovery key material must be 32 bytes" }
        val encoded = base32Encode(bytes)
        val grouped = encoded
            .chunked(GROUP_SIZE)
            .joinToString("-")
        val checksum = crc32(bytes)
        return "%s-%s-%08X".format(PREFIX, grouped, checksum)
    }

    fun parse(input: String): ByteArray {
        val trimmed = input.trim().replace(" ", "")
        val parts = trimmed.split('-')
        require(parts.size >= 3) { "Recovery key has the wrong shape" }
        require(parts[0] == PREFIX) {
            "Recovery key version \"${parts[0]}\" is not supported (expected \"$PREFIX\")"
        }
        val checksumStr = parts.last()
        require(checksumStr.length == 8) {
            "Recovery key checksum must be 8 hex characters"
        }
        val expectedChecksum = checksumStr.toLong(16).toUInt()
        val body = parts.subList(1, parts.size - 1).joinToString("")
        val bytes = base32Decode(body)
        require(bytes.size == 32) {
            "Recovery key must decode to 32 bytes, got ${bytes.size}"
        }
        val actualChecksum = crc32(bytes)
        require(actualChecksum == expectedChecksum) {
            "Recovery key checksum mismatch"
        }
        return bytes
    }

    private fun base32Encode(input: ByteArray): String {
        val output = StringBuilder()
        var buffer = 0
        var bits = 0
        for (byte in input) {
            buffer = (buffer shl 8) or (byte.toInt() and 0xff)
            bits += 8
            while (bits >= 5) {
                bits -= 5
                output.append(ALPHABET[(buffer shr bits) and 0x1f])
            }
        }
        if (bits > 0) {
            output.append(ALPHABET[(buffer shl (5 - bits)) and 0x1f])
        }
        return output.toString()
    }

    private fun base32Decode(input: String): ByteArray {
        val output = ArrayList<Byte>()
        var buffer = 0
        var bits = 0
        for (character in input) {
            val value = when (character) {
                in 'A'..'Z' -> character.code - 'A'.code
                in '2'..'7' -> character.code - '2'.code + 26
                else -> throw IllegalArgumentException("Invalid base32 character '$character'")
            }
            buffer = (buffer shl 5) or value
            bits += 5
            if (bits >= 8) {
                bits -= 8
                output.add(((buffer shr bits) and 0xff).toByte())
                buffer = buffer and ((1 shl bits) - 1)
            }
        }
        require(bits < 5) { "Invalid base32 length" }
        if (bits > 0) {
            require((buffer and ((1 shl bits) - 1)) == 0) { "Invalid base32 padding bits" }
        }
        return output.toByteArray()
    }

    private fun crc32(bytes: ByteArray): UInt {
        val crc = CRC32()
        crc.update(bytes)
        return crc.value.toUInt()
    }
}