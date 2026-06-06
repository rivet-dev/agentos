/* Open a file as O_CLOFORK and test if the flag works. */

#include "io.h"

int main(void)
{
#ifdef O_CLOFORK
	int fd = open("/dev/null", O_RDONLY | O_CLOFORK);
	if ( fd < 0 )
		err(1, "open");
	struct stat st;
	if ( fstat(fd, &st) < 0 )
		err(1, "parent fstat");
	pid_t pid = fork();
	if ( pid < 0 )
		err(1, "fork");
	if ( !pid )
	{
		if ( !fstat(fd, &st) )
			errx(1, "O_CLOFORK did not work");
		else if ( errno == EBADF )
			return 0;
		else
			err(1, "child fstat");
		
	}
	int status;
	if ( waitpid(pid, &status, 0) < 0 )
		err(1, "waitpid");
	return WIFEXITED(status) ? WEXITSTATUS(status) : 2;
#else
	errx(1, "no O_CLOFORK"); 
#endif
}
