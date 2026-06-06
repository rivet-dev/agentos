/* Test whether a basic atomic_flag_clear invocation works. */

#include <stdatomic.h>

#include "../basic.h"

int main(void)
{
	volatile atomic_flag flag;
	atomic_flag_clear(&flag);
	return 0;
}
