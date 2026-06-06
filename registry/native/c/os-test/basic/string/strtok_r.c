/* Test whether a basic strtok_r invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	char buf[8] = "abcdefg";
	char* saved;
	char* ptr = strtok_r(buf, "ce", &saved);
	if ( ptr != buf + 0 )
		errx(1, "first strtok_r did not find ab");
	if ( strcmp(ptr, "ab") != 0 )
		errx(1, "first strtok_r did not isolate ab");
	if ( memcmp(buf, "ab\0defg", 8) != 0 )
		errx(1, "first strtok_r left buffer in wrong state");
	ptr = strtok_r(NULL, "ce", &saved);
	if ( ptr != buf + 3 )
		errx(1, "second strtok_r did not find d");
	if ( strcmp(ptr, "d") != 0 )
		errx(1, "second strtok_r did not isolate d");
	if ( memcmp(buf, "ab\0d\0fg", 8) != 0 )
		errx(1, "second strtok_r left buffer in wrong state");
	ptr = strtok_r(NULL, "ce", &saved);
	if ( ptr != buf + 5 )
		errx(1, "third strtok_r did not find fg");
	if ( strcmp(ptr, "fg") != 0 )
		errx(1, "third strtok_r did not isolate fg");
	if ( memcmp(buf, "ab\0d\0fg", 8) != 0 )
		errx(1, "third strtok_r left buffer in wrong state");
	if ( strtok_r(NULL, "ce", &saved) )
		errx(1, "fourth strtok_r found something");
	return 0;
}
