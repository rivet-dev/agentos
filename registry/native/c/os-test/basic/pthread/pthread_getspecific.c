/* Test whether a basic pthread_getspecific invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_key_t tss;
	if ( (errno = pthread_key_create(&tss, NULL)) )
		err(1, "pthread_key_create");
	if ( pthread_getspecific(tss) )
		errx(1, "pthread_getspecific returned non-null");
	return 0;
}
