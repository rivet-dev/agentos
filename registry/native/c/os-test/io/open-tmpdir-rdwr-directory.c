/* Open TMPDIR for reading and writing as a directory. */

#include "io.h"

int main(void)
{
#ifdef O_DIRECTORY
	const char* tmpdir = getenv("TMPDIR") ? getenv("TMPDIR") : "/tmp";
	int fd = open(tmpdir, O_RDWR | O_DIRECTORY);
	if ( fd < 0 )
		err(1, "open");
	return 0;
#else
	errx(1, "O_DIRECTORY is not defined");
#endif
}
