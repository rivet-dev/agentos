#include <stdio.h>
#ifdef vasprintf
#undef vasprintf
#endif
int (*foo)(char **restrict, const char *restrict, va_list) = vasprintf;
int main(void) { return 0; }
