/* Test whether a basic pthread_once invocation works. */

#include <pthread.h>

#include "../basic.h"

static pthread_once_t flag = PTHREAD_ONCE_INIT;
static int calls;

static void initializer(void)
{
	calls++;
}

int main(void)
{
	if ( (errno = pthread_once(&flag, initializer)) )
		err(1, "pthread_once");
	if ( (errno = pthread_once(&flag, initializer)) )
		err(1, "pthread_once");
	if ( calls != 1 )
		errx(1, "initialized %d times", calls);
	return 0;
}
