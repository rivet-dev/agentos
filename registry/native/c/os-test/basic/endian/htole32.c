/* Test whether a basic htole32 invocation works. */

#include <endian.h>
#include <stdint.h>

#include "../basic.h"

union datum
{
	uint32_t i;
	uint8_t b[4];
};

int main(void)
{
	union datum d;
	d.i = htole32(0x01234567);
	if ( d.b[0] != 0x67 )
		errx(1, "d.b[0] != 0x67");
	if ( d.b[1] != 0x45 )
		errx(1, "d.b[1] != 0x45");
	if ( d.b[2] != 0x23 )
		errx(1, "d.b[2] != 0x23");
	if ( d.b[3] != 0x01 )
		errx(1, "d.b[3] != 0x01");
	return 0;
}
