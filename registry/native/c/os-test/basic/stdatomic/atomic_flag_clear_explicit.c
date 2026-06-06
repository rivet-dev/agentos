/* Test whether a basic atomic_flag_clear_explicit invocation works. */

#include <stdatomic.h>

#include "../basic.h"

int main(void)
{
	volatile atomic_flag flag;
	atomic_flag_clear_explicit(&flag, memory_order_seq_cst);
	return 0;
}
