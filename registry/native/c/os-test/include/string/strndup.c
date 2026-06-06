#include <string.h>
#ifdef strndup
#undef strndup
#endif
char *(*foo)(const char *, size_t) = strndup;
int main(void) { return 0; }
