/* Test whether a basic ntohl invocation works. */

#include <arpa/inet.h>
#include <inttypes.h>
#include <stdint.h>

#include "../basic.h"

union datum
{
	uint32_t i;
	uint8_t b[4];
};

int main(void)
{
	union datum d = { .b = {0x01, 0x23, 0x45, 0x67} };
	uint32_t value = ntohl(d.i);
	uint32_t expected = 0x01234567;
	if ( value != expected )
		errx(1, "got 0x%" PRIx32 " instead of 0x%" PRIx32, value, expected);
	return 0;
}

