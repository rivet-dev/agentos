/* Test whether a basic pthread_key_delete invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_key_t tss;
	if ( (errno = pthread_key_create(&tss, NULL)) )
		err(1, "pthread_key_create");
	pthread_key_delete(tss);
	return 0;
}
