/*[OB XSI]*/
/* Test whether a basic setkey invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	char key[64] = {1};
	errno = 0;
	setkey(key);
	if ( errno )
		err(1, "setkey");
	return 0;
}
