/* Test whether a basic htons invocation works. */

#include <netinet/in.h>
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
	d.i = htons(0x0123);
	if ( d.b[0] != 0x01 )
		errx(1, "d.b[0] != 0x01");
	if ( d.b[1] != 0x23 )
		errx(1, "d.b[1] != 0x23");
	return 0;
}
