/* Test whether a basic pthread_self invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	(void) pthread_self();
	return 0;
}
