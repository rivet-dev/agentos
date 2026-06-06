/* Test whether a basic call_once invocation works. */

#include <threads.h>

#include "../basic.h"

static once_flag flag = ONCE_FLAG_INIT;
static int calls;

static void initializer(void)
{
	calls++;
}

int main(void)
{
	call_once(&flag, initializer);
	call_once(&flag, initializer);
	if ( calls != 1 )
		errx(1, "initialized %d times", calls);
	return 0;
}
