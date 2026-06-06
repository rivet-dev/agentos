/* Test whether a basic fflush invocation works. */

#include <stdio.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	// Do a control test that nothing is read initially.
	char c;
	ssize_t amount = read(fileno(fp), &c, 1);
	if ( lseek(fileno(fp), 0, SEEK_SET) < 0 )
		err(1, "first lseek");
	if ( amount < 0 )
		errx(1, "first read");
	if ( 0 < amount )
		errx(1, "first read did not get eof");
	// Write a byte to a file (which will be buffered and not written yet).
	if ( fputc('x', fp) == EOF )
		err(1, "fputc");
	// Test the byte is not written the file yet.
	if ( lseek(fileno(fp), 0, SEEK_SET) < 0 )
		err(1, "second lseek");
	amount = read(fileno(fp), &c, 1);
	if ( amount < 0 )
		errx(1, "second read");
	if ( 0 < amount )
		errx(1, "second read did not get eof");
	// Flush the byte to the backing file.
	if ( fflush(fp) == EOF )
		err(1, "fflush");
	// Test the byte has been written.
	if ( lseek(fileno(fp), 0, SEEK_SET) < 0 )
		err(1, "third lseek");
	amount = read(fileno(fp), &c, 1);
	if ( amount < 0 )
		errx(1, "third read");
	if ( amount != 1 )
		errx(1, "third read did not get one byte");
	if ( c != 'x' )
		errx(1, "third read did not get 'x'");
	return 0;
}
