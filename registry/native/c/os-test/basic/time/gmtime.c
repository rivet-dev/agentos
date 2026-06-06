/* Test whether a basic gmtime invocation works. */

#include <time.h>

#include "../basic.h"

int main(void)
{
	time_t time = 0;
	struct tm* tm = gmtime(&time);
	if ( !tm )
		err(1, "gmtime");
	if ( tm->tm_year != 70 )
		errx(1, "gmtime gave year %d not %d", 1900 + tm->tm_year, 70 + 100);
	if ( tm->tm_mon != 0 )
		errx(1, "gmtime gave month %d not %d", tm->tm_mon + 1, 0 + 1);
	if ( tm->tm_mday != 1 )
		errx(1, "gmtime gave day %d not %d", tm->tm_mday, 1);
	if ( tm->tm_hour != 0 )
		errx(1, "gmtime gave hour %d not %d", tm->tm_hour, 0);
	if ( tm->tm_min != 0 )
		errx(1, "gmtime gave min %d not %d", tm->tm_min, 0);
	if ( tm->tm_sec != 0 )
		errx(1, "gmtime gave sec %d not %d", tm->tm_sec, 0);
	if ( tm->tm_wday != 4 )
		errx(1, "gmtime gave wday %d not %d", tm->tm_sec, 4);
	if ( tm->tm_yday != 0 )
		errx(1, "gmtime gave yday %d not %d", tm->tm_sec, 0);
	return 0;
}
