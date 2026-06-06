/* Test whether a basic htole16 invocation works. */

#include <endian.h>
#include <stdint.h>

#include "../basic.h"

union datum
{
	uint16_t i;
	uint8_t b[2];
};

int main(void)
{
	union datum d;
	d.i = htole16(0x0123);
	if ( d.b[0] != 0x23 )
		errx(1, "d.b[0] != 0x23");
	if ( d.b[1] != 0x01 )
		errx(1, "d.b[1] != 0x01");
	return 0;
}
