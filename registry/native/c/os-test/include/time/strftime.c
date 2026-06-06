#include <time.h>
#ifdef strftime
#undef strftime
#endif
size_t (*foo)(char *restrict, size_t, const char *restrict, const struct tm *restrict) = strftime;
int main(void) { return 0; }
