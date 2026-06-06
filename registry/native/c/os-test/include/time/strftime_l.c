#include <time.h>
#ifdef strftime_l
#undef strftime_l
#endif
size_t (*foo)(char *restrict, size_t, const char *restrict, const struct tm *restrict, locale_t) = strftime_l;
int main(void) { return 0; }
