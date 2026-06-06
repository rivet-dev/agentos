/* Test whether a basic strtok invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	char buf[8] = "abcdefg";
	char* ptr = strtok(buf, "ce");
	if ( ptr != buf + 0 )
		errx(1, "first strtok did not find ab");
	if ( strcmp(ptr, "ab") != 0 )
		errx(1, "first strtok did not isolate ab");
	if ( memcmp(buf, "ab\0defg", 8) != 0 )
		errx(1, "first strtok left buffer in wrong state");
	ptr = strtok(NULL, "ce");
	if ( ptr != buf + 3 )
		errx(1, "second strtok did not find d");
	if ( strcmp(ptr, "d") != 0 )
		errx(1, "second strtok did not isolate d");
	if ( memcmp(buf, "ab\0d\0fg", 8) != 0 )
		errx(1, "second strtok left buffer in wrong state");
	ptr = strtok(NULL, "ce");
	if ( ptr != buf + 5 )
		errx(1, "third strtok did not find fg");
	if ( strcmp(ptr, "fg") != 0 )
		errx(1, "third strtok did not isolate fg");
	if ( memcmp(buf, "ab\0d\0fg", 8) != 0 )
		errx(1, "third strtok left buffer in wrong state");
	if ( strtok(NULL, "ce") )
		errx(1, "fourth strtok found something");
	return 0;
}
