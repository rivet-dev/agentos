/* Test whether a basic htonl invocation works. */

#include <arpa/inet.h>
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
	d.i = htonl(0x01234567);
	if ( d.b[0] != 0x01 )
		errx(1, "d.b[0] != 0x01");
	if ( d.b[1] != 0x23 )
		errx(1, "d.b[1] != 0x23");
	if ( d.b[2] != 0x45 )
		errx(1, "d.b[2] != 0x45");
	if ( d.b[3] != 0x67 )
		errx(1, "d.b[3] != 0x67");
	return 0;
}

