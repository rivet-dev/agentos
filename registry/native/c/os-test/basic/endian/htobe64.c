/* Test whether a basic htobe64 invocation works. */

#include <endian.h>
#include <stdint.h>

#include "../basic.h"

union datum
{
	uint64_t i;
	uint8_t b[8];
};

int main(void)
{
	union datum d;
	d.i = be64toh(0x0123456789abcdef);
	if ( d.b[0] != 0x01 )
		errx(1, "d.b[0] != 0x01");
	if ( d.b[1] != 0x23 )
		errx(1, "d.b[1] != 0x23");
	if ( d.b[2] != 0x45 )
		errx(1, "d.b[2] != 0x45");
	if ( d.b[3] != 0x67 )
		errx(1, "d.b[3] != 0x67");
	if ( d.b[4] != 0x89 )
		errx(1, "d.b[4] != 0x89");
	if ( d.b[5] != 0xab )
		errx(1, "d.b[5] != 0xab");
	if ( d.b[6] != 0xcd )
		errx(1, "d.b[6] != 0xcd");
	if ( d.b[7] != 0xef )
		errx(1, "d.b[7] != 0xef");
	return 0;
}
