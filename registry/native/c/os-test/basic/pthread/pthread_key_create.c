/* Test whether a basic pthread_key_create invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_key_t tss;
	if ( (errno = pthread_key_create(&tss, NULL)) )
		err(1, "pthread_key_create");
	return 0;
}
