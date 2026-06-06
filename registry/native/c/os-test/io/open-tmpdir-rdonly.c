/* Open TMPDIR for reading. */

#include "io.h"

int main(void)
{
	const char* tmpdir = getenv("TMPDIR") ? getenv("TMPDIR") : "/tmp";
	int fd = open(tmpdir, O_RDONLY);
	if ( fd < 0 )
		err(1, "open");
	return 0;
}
