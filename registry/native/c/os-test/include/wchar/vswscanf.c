#include <wchar.h>
#ifdef vswscanf
#undef vswscanf
#endif
int (*foo)(const wchar_t *restrict, const wchar_t *restrict, va_list) = vswscanf;
int main(void) { return 0; }
