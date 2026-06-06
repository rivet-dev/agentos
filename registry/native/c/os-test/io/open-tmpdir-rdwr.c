/* Open TMPDIR for reading and writing. */

#include "io.h"

int main(void)
{
	const char* tmpdir = getenv("TMPDIR") ? getenv("TMPDIR") : "/tmp";
	int fd = open(tmpdir, O_RDWR);
	if ( fd < 0 )
		err(1, "open");
	return 0;
}
