#include <monetary.h>
#ifdef strfmon_l
#undef strfmon_l
#endif
ssize_t (*foo)(char *restrict, size_t, locale_t, const char *restrict, ...) = strfmon_l;
int main(void) { return 0; }
