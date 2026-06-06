/* Open TMPDIR for reading and appending. */

#include "io.h"

int main(void)
{
	const char* tmpdir = getenv("TMPDIR") ? getenv("TMPDIR") : "/tmp";
	int fd = open(tmpdir, O_RDONLY | O_APPEND);
	if ( fd < 0 )
		err(1, "open");
	return 0;
}
