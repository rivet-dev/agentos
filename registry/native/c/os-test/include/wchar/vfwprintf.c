#include <wchar.h>
#ifdef vfwprintf
#undef vfwprintf
#endif
int (*foo)(FILE *restrict, const wchar_t *restrict, va_list) = vfwprintf;
int main(void) { return 0; }
