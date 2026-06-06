/* Test whether a basic sem_open invocation works. */

#include <fcntl.h>
#include <limits.h>
#include <semaphore.h>
#include <stdbool.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	long counter = (long) getpid();
	while ( true )
	{
		char path[255];
		snprintf(path, sizeof(path), "/os_test_sem_open.%li", counter);
		sem_t* sem = sem_open(path, O_CREAT | O_EXCL, 0600, 1);
		if ( !sem )
		{
			if ( errno != EEXIST )
				errx(1, "sem_open: %s", path);
			counter = counter != LONG_MAX ? counter + 1 : 0;
			continue;
		}
		sem_close(sem);
		sem_unlink(path);
		break;
	}
	return 0;
}
