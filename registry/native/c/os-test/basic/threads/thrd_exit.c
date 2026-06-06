/* Test whether a basic thrd_exit invocation works. */

#include <threads.h>

#include "../basic.h"

int main(void)
{
	thrd_exit(0);
}
