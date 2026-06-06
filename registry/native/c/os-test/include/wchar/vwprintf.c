#include <wchar.h>
#ifdef vwprintf
#undef vwprintf
#endif
int (*foo)(const wchar_t *restrict, va_list) = vwprintf;
int main(void) { return 0; }
