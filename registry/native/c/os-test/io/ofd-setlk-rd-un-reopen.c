/* Lock a temporary file with F_OFD_SETLK for reading and unlock and get the
   lock status after opening the file again. */

#include "io.h"

int main(void)
{
#ifdef F_OFD_SETLK
	const char* tmpdir = getenv("TMPDIR") ? getenv("TMPDIR") : "/tmp";
	const char* template = "ofd-setlk.XXXXXX";
	size_t path_size = strlen(tmpdir) + 1 + strlen(template) + 1;
	char* path = malloc(path_size);
	if ( !path )
		err(1, "malloc");
	snprintf(path, path_size, "%s/%s", tmpdir, template);
	int tmp_fd = mkstemp(path);
	if ( tmp_fd < 0 )
		err(1, "mkstemp");
	int fd = open(path, O_RDWR);
	if ( fd < 0 )
	{
		unlink(path);
		err(1, "open");
	}
	struct flock lock = { .l_type = F_RDLCK };
	if ( fcntl(fd, F_OFD_SETLK, &lock) < 0 )
	{
		unlink(path);
		err(1, "fcntl: F_OFD_SETLK:_F_RDLCK");
	}
	struct flock unlock = { .l_type = F_UNLCK };
	if ( fcntl(fd, F_OFD_SETLK, &unlock) < 0 )
	{
		unlink(path);
		err(1, "fcntl: F_OFD_SETLK: F_UNLCK");
	}
	int fd2 = open(path, O_RDWR);
	if ( fd2 < 0 )
	{
		unlink(path);
		err(1, "reopen");
	}
	struct flock outcome = { .l_type = F_WRLCK };
	if ( fcntl(fd2, F_OFD_GETLK, &outcome) < 0 )
	{
		unlink(path);
		err(1, "fcntl: F_OFD_GETLK");
	}
	if ( outcome.l_type == F_UNLCK )
		printf("F_UNLCK\n");
	else if ( outcome.l_type == F_RDLCK )
		printf("F_RDLCK\n");
	else if ( outcome.l_type == F_WRLCK )
		printf("F_WRLCK\n");
	else
		printf("%#x\n", outcome.l_type);
	unlink(path);
	return 0;
#else
	errx(1, "no F_OFD_SETLK"); 
#endif
}
