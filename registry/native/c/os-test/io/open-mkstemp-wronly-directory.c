/* Open a temporary file for writing as a directory, testing whether the open
   succeeds. */

#include "io.h"

int main(void)
{
#ifdef O_DIRECTORY
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
	int fd = open(path, O_WRONLY | O_DIRECTORY);
	if ( fd < 0 )
	{
		unlink(path);
		err(1, "open");
	}
	unlink(path);
	return 0;
#else
	errx(1, "O_DIRECTORY is not defined");
#endif
}
