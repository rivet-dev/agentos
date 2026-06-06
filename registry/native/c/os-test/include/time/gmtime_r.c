#include <time.h>
#ifdef gmtime_r
#undef gmtime_r
#endif
struct tm *(*foo)(const time_t *restrict, struct tm *restrict) = gmtime_r;
int main(void) { return 0; }
