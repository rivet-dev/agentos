/*[OB XSI]*/
/* Test whether a basic encrypt invocation works. */

#include <errno.h>
#include <stdlib.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	char key[64] = {1};
	char buffer[64] = {0};
	errno = 0;
	setkey(key);
	if ( errno )
		err(1, "setkey");
	errno = 0;
	encrypt(buffer, 0);
	if ( errno )
		err(1, "encrypt");
	return 0;
}
