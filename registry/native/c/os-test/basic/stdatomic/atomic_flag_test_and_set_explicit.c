/* Test whether a basic atomic_flag_test_and_set_explicit invocation works. */

#include <stdatomic.h>

#include "../basic.h"

int main(void)
{
	volatile atomic_flag flag;
	atomic_flag_clear(&flag);
	if ( atomic_flag_test_and_set_explicit(&flag, memory_order_seq_cst) )
		errx(1, "first: flag was set, not clear");
	if ( !atomic_flag_test_and_set_explicit(&flag, memory_order_seq_cst) )
		errx(1, "second: flag was clear, not set");
	if ( !atomic_flag_test_and_set_explicit(&flag, memory_order_seq_cst) )
		errx(1, "third: flag was clear, not set");
	return 0;
}
