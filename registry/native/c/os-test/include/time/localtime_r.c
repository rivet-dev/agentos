#include <time.h>
#ifdef localtime_r
#undef localtime_r
#endif
struct tm *(*foo)(const time_t *restrict, struct tm *restrict) = localtime_r;
int main(void) { return 0; }
