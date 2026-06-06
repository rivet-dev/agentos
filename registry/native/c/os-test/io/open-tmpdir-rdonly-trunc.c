/* Open TMPDIR for reading and truncation. */

#include "io.h"

int main(void)
{
	const char* tmpdir = getenv("TMPDIR") ? getenv("TMPDIR") : "/tmp";
	int fd = open(tmpdir, O_RDONLY | O_TRUNC);
	if ( fd < 0 )
		err(1, "open");
	return 0;
}
