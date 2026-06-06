/* Test whether a basic _Exit invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	_Exit(0);
	return 1;
}
