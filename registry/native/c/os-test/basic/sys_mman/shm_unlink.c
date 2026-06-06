/*[SHM]*/
/* Test whether a basic shm_unlink invocation works. */

#include <sys/mman.h>

#include <fcntl.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	// Generate random file names with mkstemp until shm_open succeeds.
	const char* tmpdir = getenv("TMPDIR");
	if ( !tmpdir )
		tmpdir = "/tmp";
	size_t template_len = strlen(tmpdir) + strlen("/os-test.XXXXXX");
	char* template = malloc(template_len + 1);
	if ( !template )
		err(1, "malloc");
	int fd;
	char* shm_name;
	while ( 1 )
	{
		strcpy(template, tmpdir);
		strcat(template, "/os-test.XXXXXX");
		int tmp_fd = mkstemp(template);
		if ( tmp_fd < 0 )
			err(1, "mkstemp");
		close(tmp_fd);
		unlink(template);
		shm_name = template + strlen(tmpdir);
		fd = shm_open(shm_name, O_RDWR | O_CREAT | O_EXCL, 0600);
		if ( fd < 0  )
		{
			if ( errno == EEXIST )
				continue;
			err(1, "shm_open");
		}
		break;
	}
	// Test deleting the shared memory object.
	if ( shm_unlink(shm_name) < 0 )
		err(1, "shm_unlink");
	return 0;
}
