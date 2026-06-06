#include <monetary.h>
#ifdef strfmon
#undef strfmon
#endif
ssize_t (*foo)(char *restrict, size_t, const char *restrict, ...) = strfmon;
int main(void) { return 0; }
