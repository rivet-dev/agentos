#include <wchar.h>
#ifdef wprintf
#undef wprintf
#endif
int (*foo)(const wchar_t *restrict, ...) = wprintf;
int main(void) { return 0; }
