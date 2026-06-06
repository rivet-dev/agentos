/*[XSI]*/
/* Test whether a basic gethostid invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	gethostid();
	return 0;
}
