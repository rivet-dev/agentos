/*[XSI]*/
/* Test whether a basic crypt invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	char* hashed = crypt("foo", "ba");
	if ( !hashed )
		err(1, "crypt");
	return 0;
}
