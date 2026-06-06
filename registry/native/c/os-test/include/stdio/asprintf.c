#include <stdio.h>
#ifdef asprintf
#undef asprintf
#endif
int (*foo)(char **restrict, const char *restrict, ...) = asprintf;
int main(void) { return 0; }
