#include <wchar.h>
#ifdef vswprintf
#undef vswprintf
#endif
int (*foo)(wchar_t *restrict, size_t, const wchar_t *restrict, va_list) = vswprintf;
int main(void) { return 0; }
