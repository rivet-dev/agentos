/*[XSI]*/
/* Test whether a basic srandom invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	srandom(42);
	return 0;
}
