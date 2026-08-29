import SwiftUI

struct ContentView: View {
    @StateObject private var vm = ChatViewModel()
    @State private var tokenInput: String = ""

    var body: some View {
        NavigationStack {
            VStack(spacing: 12) {
                Form {
                    Section("Conexion") {
                        TextField("Base URL", text: $vm.baseURL)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                        TextField("Model", text: $vm.model)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                        TextField("Session ID", text: $vm.sessionId)
                        TextField("User ID", text: $vm.userId)
                        TextField("Project ID", text: $vm.projectId)

                        SecureField("Bearer token", text: $tokenInput)
                        Button("Guardar token seguro") {
                            vm.saveToken(tokenInput)
                            tokenInput = ""
                        }
                    }
                }
                .frame(maxHeight: 320)

                if let err = vm.errorText {
                    Text(err)
                        .font(.footnote)
                        .foregroundStyle(.red)
                        .padding(.horizontal)
                }

                List(vm.items) { item in
                    VStack(alignment: .leading, spacing: 4) {
                        Text(item.role.uppercased())
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        Text(item.text)
                            .font(.body)
                        if let meta = item.meta {
                            Text(meta)
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                        }
                    }
                }

                HStack {
                    TextField("Escribe para Quiron...", text: $vm.inputText)
                        .textFieldStyle(.roundedBorder)

                    Button(vm.isSending ? "..." : "Enviar") {
                        Task { await vm.send() }
                    }
                    .disabled(vm.isSending)
                }
                .padding(.horizontal)
                .padding(.bottom, 10)
            }
            .navigationTitle("Quiron iPhone")
        }
    }
}

#Preview {
    ContentView()
}
