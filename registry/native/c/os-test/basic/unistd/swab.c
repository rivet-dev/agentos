/*[XSI]*/
/* Test whether a basic swab invocation works. */

#include <string.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	char in[5] = "abcd";
	char out[5];
	swab(in, out, 4);
	out[4] = '\0';
	if ( strcmp(out, "badc") != 0 )
		errx(1, "swab did not swap bytes");
	return 0;
}
