/*[XSI]*/
/* Test whether a basic l64a invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	char* string = l64a(9854977);
	if ( !string )
		errx(1, "l64a returned NULL");
	if ( strcmp(string, "/.aZ") != 0 )
		errx(1, "l64a was \"%s\", not \"%s\"", string, "/.aZ");
	return 0;
}
