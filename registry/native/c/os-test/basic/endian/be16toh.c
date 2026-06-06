/* Test whether a basic be16toh invocation works. */

#include <endian.h>
#include <inttypes.h>
#include <stdint.h>

#include "../basic.h"

union datum
{
	uint16_t i;
	uint8_t b[2];
};

int main(void)
{
	union datum d = { .b = {0x01, 0x23} };
	uint16_t value = be16toh(d.i);
	uint16_t expected = 0x0123;
	if ( value != expected )
		errx(1, "got 0x%" PRIx16 " instead of 0x%" PRIx16, value, expected);
	return 0;
}
