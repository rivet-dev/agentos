/*[XSI]*/
/* Test whether a basic sync invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	sync();
	return 0;
}
