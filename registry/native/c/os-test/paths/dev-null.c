/* Test whether /dev/null exists and contains no data. */

#include "suite.h"

int main(void)
{
	// POSIX requires /dev/null to exist.
	// Test the device exists and can be opened for reading and writing.
	const char* path = "/dev/null";
	int fd = open(path, O_RDWR);
	if ( fd < 0 )
		err(1, "%s", path);
	char c;
	// Test the device doesn't contain data.
	ssize_t amount = read(fd, &c, 1);
	if ( amount < 0 )
		err(1, "first read");
	if ( amount != 0 )
		errx(1, "read() != 0");
	c = 'x';
	// Test that data can be written to the device.
	amount = write(fd, &c, 1);
	if ( amount < 0 )
		err(1, "write");
	if ( amount != 1 )
		errx(1, "write() != 1");
	// Test that the device is seekable.
	// TODO: Interesting. /dev/null has an offset on half of the systems.
	//off_t offset = lseek(fd, 0, SEEK_CUR);
	//if ( offset < 0 )
	//	errx(1, "lseek");
	//if ( offset != 1 )
	//	err(1, "lseek() != 1");
	if ( lseek(fd, 0, SEEK_SET) < 0 )
		err(1, "lseek");
	// Test that the written data is not read again.
	amount = read(fd, &c, 1);
	if ( amount < 0 )
		err(1, "second read");
	if ( amount != 0 )
		errx(1, "second read() != 0");
	puts("Yes");
	return 0;
}
