/*[SHM]*/
/* Test whether a basic shm_open invocation works. */

#include <sys/mman.h>

#include <fcntl.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "../basic.h"

static const char* temporary;

static void cleanup(void)
{
	if ( temporary )
		shm_unlink(temporary);
}

int main(void)
{
	// Generate random file names with mkstemp until shm_open succeeds.
	if ( atexit(cleanup) )
		err(1, "atexit");
	const char* tmpdir = getenv("TMPDIR");
	if ( !tmpdir )
		tmpdir = "/tmp";
	size_t template_len = strlen(tmpdir) + strlen("/os-test.XXXXXX");
	char* template = malloc(template_len + 1);
	if ( !template )
		err(1, "malloc");
	int fd;
	while ( 1 )
	{
		strcpy(template, tmpdir);
		strcat(template, "/os-test.XXXXXX");
		int tmp_fd = mkstemp(template);
		if ( tmp_fd < 0 )
			err(1, "mkstemp");
		close(tmp_fd);
		unlink(template);
		char* shm_name = template + strlen(tmpdir);
		fd = shm_open(shm_name, O_RDWR | O_CREAT | O_EXCL, 0600);
		if ( fd < 0  )
		{
			if ( errno == EEXIST )
				continue;
			err(1, "shm_open");
		}
		temporary = shm_name;
		break;
	}
	// Test if the shared memory file can be mapped.
	long pagesize = sysconf(_SC_PAGESIZE);
	if ( pagesize < 0 )
		err(1, "sysconf _SC_PAGESIZE");
	if ( ftruncate(fd, pagesize) < 0 )
		err(1, "ftruncate");
	char* ptr = mmap(NULL, pagesize, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
	if ( ptr == MAP_FAILED )
		err(1, "mmap");
	return 0;
}
