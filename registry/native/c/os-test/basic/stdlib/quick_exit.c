/* Test whether a basic quick_exit invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	quick_exit(0);
	err(1, "quick_exit did not exit");
}
