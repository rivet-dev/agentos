/* Test whether a basic be64toh invocation works. */

#include <endian.h>
#include <inttypes.h>
#include <stdint.h>

#include "../basic.h"

union datum
{
	uint64_t i;
	uint8_t b[8];
};

int main(void)
{
	union datum d = { .b = {0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef} };
	uint64_t value = be64toh(d.i);
	uint64_t expected = 0x0123456789abcdef;
	if ( value != expected )
		errx(1, "got 0x%" PRIx64 " instead of 0x%" PRIx64, value, expected);
	return 0;
}
