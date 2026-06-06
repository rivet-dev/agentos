/* Test whether a basic _exit invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	_exit(0);
}
