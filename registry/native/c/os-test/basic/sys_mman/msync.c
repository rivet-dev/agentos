/*[XSI|SIO]*/
/* Test whether a basic msync invocation works. */

#include <sys/mman.h>

#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "../basic.h"

static const char* temporary;

static void cleanup(void)
{
	if ( temporary )
		unlink(temporary);
}

int main(void)
{
	if ( atexit(cleanup) )
		err(1, "atexit");
	const char* tmpdir = getenv("TMPDIR");
	if ( !tmpdir )
		tmpdir = "/tmp";
	size_t template_len = strlen(tmpdir) + strlen("/os-test.XXXXXX");
	char* template = malloc(template_len + 1);
	if ( !template )
		err(1, "malloc");
	strcpy(template, tmpdir);
	strcat(template, "/os-test.XXXXXX");
	int fd = mkstemp(template);
	if ( fd < 0 )
		err(1, "mkstemp");
	temporary = template;
	long pagesize = sysconf(_SC_PAGESIZE);
	if ( pagesize < 0 )
		err(1, "sysconf _SC_PAGESIZE");
	if ( ftruncate(fd, pagesize) < 0 )
		err(1, "ftruncate");
	char* ptr = mmap(NULL, pagesize, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
	if ( ptr == MAP_FAILED )
		err(1, "mmap");
	ptr[0] = 'x';
	if ( msync(ptr, pagesize, MS_SYNC) < 0 )
		err(1, "msync");
	char c;
	if ( read(fd, &c, 1) != 1 )
		err(1, "read");
	if ( c != 'x' )
		errx(1, "msync did not sync");
	return 0;
}
