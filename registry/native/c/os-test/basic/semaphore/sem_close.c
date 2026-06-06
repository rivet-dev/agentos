/* Test whether a basic sem_close invocation works. */

#include <fcntl.h>
#include <limits.h>
#include <semaphore.h>
#include <stdbool.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	int ret = 0;
	long counter = (long) getpid();
	while ( true )
	{
		char path[255];
		snprintf(path, sizeof(path), "/os_test_sem_close.%li", counter);
		sem_t* sem = sem_open(path, O_CREAT | O_EXCL, 0600, 1);
		if ( !sem )
		{
			if ( errno != EEXIST )
				errx(1, "sem_open: %s", path);
			counter = counter != LONG_MAX ? counter + 1 : 0;
			continue;
		}
		if ( sem_close(sem) < 0 )
		{
			warn("sem_close");
			ret = 1;
		}
		sem_unlink(path);
		break;
	}
	return ret;
}
