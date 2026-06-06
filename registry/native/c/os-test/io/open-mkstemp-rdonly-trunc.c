/* Open a temporary file for reading and truncation, testing whether there are
   any unintended truncation. */

#include "io.h"

int main(void)
{
	const char* tmpdir = getenv("TMPDIR") ? getenv("TMPDIR") : "/tmp";
	const char* template = "open-mkstemp-rdonly.XXXXXX";
	size_t path_size = strlen(tmpdir) + 1 + strlen(template) + 1;
	char* path = malloc(path_size);
	if ( !path )
		err(1, "malloc");
	snprintf(path, path_size, "%s/%s", tmpdir, template);
	int tmp_fd = mkstemp(path);
	if ( tmp_fd < 0 )
		err(1, "mkstemp");
	char x = 'x';
	if ( write(tmp_fd, &x, 1) < 0 )
		err(1, "write");
	int fd = open(path, O_RDONLY | O_TRUNC);
	off_t size = lseek(tmp_fd, 0, SEEK_END);
	if ( size != 1 )
		printf("file was truncated\n");
	if ( fd < 0 )
	{
		unlink(path);
		err(1, "open");
	}
	unlink(path);
	return 0;
}
