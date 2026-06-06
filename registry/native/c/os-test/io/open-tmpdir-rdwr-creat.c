/* Open TMPDIR for reading, writing, and creation. */

#include "io.h"

int main(void)
{
	const char* tmpdir = getenv("TMPDIR") ? getenv("TMPDIR") : "/tmp";
	int fd = open(tmpdir, O_RDWR | O_CREAT, 0777);
	if ( fd < 0 )
		err(1, "open");
	return 0;
}
