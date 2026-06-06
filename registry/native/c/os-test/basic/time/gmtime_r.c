/* Test whether a basic gmtime_r invocation works. */

#include <time.h>

#include "../basic.h"

int main(void)
{
	time_t time = 0;
	struct tm storage;
	struct tm* tm = gmtime_r(&time, &storage);
	if ( !tm )
		err(1, "gmtime_r");
	if ( tm->tm_year != 70 )
		errx(1, "gmtime_r gave year %d not %d", 1900 + tm->tm_year, 70 + 100);
	if ( tm->tm_mon != 0 )
		errx(1, "gmtime_r gave month %d not %d", tm->tm_mon + 1, 0 + 1);
	if ( tm->tm_mday != 1 )
		errx(1, "gmtime_r gave day %d not %d", tm->tm_mday, 1);
	if ( tm->tm_hour != 0 )
		errx(1, "gmtime_r gave hour %d not %d", tm->tm_hour, 0);
	if ( tm->tm_min != 0 )
		errx(1, "gmtime_r gave min %d not %d", tm->tm_min, 0);
	if ( tm->tm_sec != 0 )
		errx(1, "gmtime_r gave sec %d not %d", tm->tm_sec, 0);
	if ( tm->tm_wday != 4 )
		errx(1, "gmtime_r gave wday %d not %d", tm->tm_sec, 4);
	if ( tm->tm_yday != 0 )
		errx(1, "gmtime_r gave yday %d not %d", tm->tm_sec, 0);
	return 0;
}
