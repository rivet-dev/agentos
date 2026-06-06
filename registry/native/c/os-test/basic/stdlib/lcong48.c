/*[XSI]*/
/* Test whether a basic lcong48 invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	unsigned short params[7] =
	{
		42,
		1337,
		9001,
		0x0000,
		0x5DEE,
		0xE66D,
		0x000B,
	};
	lcong48(params);
	return 0;
}
